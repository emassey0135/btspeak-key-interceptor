use bitflags::{bitflags, Flags};
use evdev::{enumerate, EventSummary, KeyCode};
use evdev::uinput::VirtualDevice;
use std::cell::Cell;
use std::sync::Arc;
use tokio::select;
use tokio::sync::{mpsc, Mutex, oneshot};
use tokio_stream::wrappers::ReceiverStream;
use tokio_util::sync::CancellationToken;
use tonic::{transport::Server, Request, Response, Status};
use btspeak_key_interceptor::btspeak_key_interceptor_server::{BtspeakKeyInterceptor, BtspeakKeyInterceptorServer};
use btspeak_key_interceptor::{Empty, BrailleKeyCombination, BrailleKeyCombinations, BrailleKeyEvent, BrailleKeyEvents};
mod btspeak_key_interceptor {
  tonic::include_proto!("btspeak_key_interceptor");
}
bitflags! {
  #[derive(Debug, PartialEq, Eq, Clone)]
  struct KeyFlags: u16 {
    const Dot1 = 1;
    const Dot2 = 1 << 1;
    const Dot3 = 1 << 2;
    const Dot4 = 1 << 3;
    const Dot5 = 1 << 4;
    const Dot6 = 1 << 5;
    const Dot7 = 1 << 6;
    const Dot8 = 1 << 7;
    const Space = 1 << 8;
  }
}
#[derive(Debug)]
struct State {
  sending_key_combinations: bool,
  sending_key_events: bool,
}
struct MyServer {
  state: Arc<Mutex<State>>,
  combination_rx: Arc<Mutex<mpsc::Receiver<KeyFlags>>>,
  combination_sender: Mutex<Cell<Option<(CancellationToken, oneshot::Receiver<()>)>>>,
  event_rx: Arc<Mutex<mpsc::Receiver<(KeyFlags, bool)>>>,
  event_sender: Mutex<Cell<Option<(CancellationToken, oneshot::Receiver<()>)>>>,
}
#[tonic::async_trait]
impl BtspeakKeyInterceptor for MyServer {
  type GrabKeyCombinationsStream = ReceiverStream<Result<BrailleKeyCombination, Status>>;
  async fn grab_key_combinations(&self, request: Request<Empty>) -> Result<Response<Self::GrabKeyCombinationsStream>, Status> {
    let combination_sender = self.combination_sender.lock().await;
    self.state.lock().await.sending_key_combinations = true;
    let (tx, rx) = mpsc::channel(32);
    let combination_rx = self.combination_rx.clone();
    let canceller = CancellationToken::new();
    let (cancel_tx, cancel_rx) = oneshot::channel();
    combination_sender.set(Some((canceller.clone(), cancel_rx)));
    tokio::spawn(async move {
      loop {
        let mut combination_rx = combination_rx.lock().await;
        select! {
          _ = canceller.cancelled() => {
            cancel_tx.send(()).unwrap();
            break;
          },
          result = combination_rx.recv() => {
            match result {
              Some(combination) => {
                let dots = (combination.clone() - KeyFlags::Space).bits();
                let space = combination.contains(KeyFlags::Space);
                let combination = BrailleKeyCombination { dots: dots as i32, space };
                tx.send(Ok(combination)).await.unwrap();
              },
              None => {
                cancel_tx.send(()).unwrap();
                break;
              },
            };
          },
        };
      };
    });
    Ok(Response::new(ReceiverStream::new(rx)))
  }
  type GrabKeyEventsStream = ReceiverStream<Result<BrailleKeyEvent, Status>>;
  async fn grab_key_events(&self, request: Request<Empty>) -> Result<Response<Self::GrabKeyEventsStream>, Status> {
    let event_sender = self.event_sender.lock().await;
    self.state.lock().await.sending_key_events = true;
    let (tx, rx) = mpsc::channel(32);
    let event_rx = self.event_rx.clone();
    let canceller = CancellationToken::new();
    let (cancel_tx, cancel_rx) = oneshot::channel();
    event_sender.set(Some((canceller.clone(), cancel_rx)));
    tokio::spawn(async move {
      loop {
        let mut event_rx = event_rx.lock().await;
        select! {
          _ = canceller.cancelled() => {
            cancel_tx.send(()).unwrap();
            break;
          },
          result = event_rx.recv() => {
            match result {
              Some(event) => {
                let dot = event.0.bits();
                let release = event.1;
                let event = BrailleKeyEvent { dot: dot as i32, release };
                tx.send(Ok(event)).await.unwrap();
              },
              None => {
                cancel_tx.send(()).unwrap();
                break;
              },
            };
          },
        };
      };
    });
    Ok(Response::new(ReceiverStream::new(rx)))
  }
  async fn set_excluded_key_combinations(&self, request: Request<BrailleKeyCombinations>) -> Result<Response<Empty>, Status> {
    let reply = Empty {};
    Ok(Response::new(reply))
  }
  async fn set_excluded_key_events(&self, request: Request<BrailleKeyEvents>) -> Result<Response<Empty>, Status> {
    let reply = Empty {};
    Ok(Response::new(reply))
  }
  async fn release_keyboard(&self, _request: Request<Empty>) -> Result<Response<Empty>, Status> {
    let mut state = self.state.lock().await;
    let combination_sender = self.combination_sender.lock().await;
    let event_sender = self.event_sender.lock().await;
    if let Some((canceller, cancel_rx)) = combination_sender.take() {
      canceller.cancel();
      cancel_rx.await.unwrap();
    };
    if let Some((canceller, cancel_rx)) = event_sender.take() {
      canceller.cancel();
      cancel_rx.await.unwrap();
    };
    state.sending_key_combinations = false;
    state.sending_key_events = false;
    let reply = Empty {};
    Ok(Response::new(reply))
  }
  async fn send_key_combination(&self, request: Request<BrailleKeyCombination>) -> Result<Response<Empty>, Status> {
    let reply = Empty {};
    Ok(Response::new(reply))
  }
  async fn send_key_event(&self, request: Request<BrailleKeyEvent>) -> Result<Response<Empty>, Status> {
    let reply = Empty {};
    Ok(Response::new(reply))
  }
}
#[tokio::main]
async fn main() {
  let mut device = enumerate()
    .find(|(_, device)| device.name().map_or(false, |name| name=="4x3braille"))
    .unwrap()
    .1;
  device.grab().unwrap();
  let mut virtual_device = VirtualDevice::builder()
    .unwrap()
    .name("btspeak-key-interceptor")
    .with_keys(device.supported_keys().unwrap())
    .unwrap()
    .build()
    .unwrap();
  let mut event_stream = device.into_event_stream().unwrap();
  let state = Arc::new(Mutex::new(State { sending_key_combinations: false, sending_key_events: false }));
  let state2 = state.clone();
  let (combination_tx, combination_rx) = mpsc::channel(32);
  let (event_tx, event_rx) = mpsc::channel(32);
  tokio::spawn(async move {
    let mut pressed_keys = KeyFlags::empty();
    let mut held_keys = KeyFlags::empty();
    while let Ok(event) = event_stream.next_event().await {
      match event.destructure() {
        EventSummary::Key(_, code, value) => {
          println!("Key {:?}, value {}", code, value);
          let flag = match code {
            KeyCode::KEY_BRL_DOT1 => KeyFlags::Dot1,
            KeyCode::KEY_BRL_DOT2 => KeyFlags::Dot2,
            KeyCode::KEY_BRL_DOT3 => KeyFlags::Dot3,
            KeyCode::KEY_BRL_DOT4 => KeyFlags::Dot4,
            KeyCode::KEY_BRL_DOT5 => KeyFlags::Dot5,
            KeyCode::KEY_BRL_DOT6 => KeyFlags::Dot6,
            KeyCode::KEY_BRL_DOT7 => KeyFlags::Dot7,
            KeyCode::KEY_BRL_DOT8 => KeyFlags::Dot8,
            KeyCode::KEY_SPACE => KeyFlags::Space,
            _ => KeyFlags::empty(),
          };
          match value {
            0 => held_keys -= flag.clone(),
            _ => {
              pressed_keys |= flag.clone();
              held_keys |= flag.clone();
            },
          };
          let state = state.lock().await;
          if held_keys==KeyFlags::empty() {
            if state.sending_key_combinations && pressed_keys!=KeyFlags::empty() {
              combination_tx.send(pressed_keys.clone()).await.unwrap();
            };
            pressed_keys.clear();
          };
          if state.sending_key_events {
            let released = value==0;
            event_tx.send((flag.clone(), released)).await.unwrap();
          };
          if !state.sending_key_combinations && !state.sending_key_events {
            virtual_device.emit(&[event]).unwrap();
          };
        },
        _ => {}
      };
    };
  });
  let addr = "127.0.0.1:54123".parse().unwrap();
  let server = MyServer {
    state: state2,
    combination_rx: Arc::new(Mutex::new(combination_rx)),
    combination_sender: Mutex::new(Cell::new(None)),
    event_rx: Arc::new(Mutex::new(event_rx)),
    event_sender: Mutex::new(Cell::new(None)),
  };
  Server::builder()
    .add_service(BtspeakKeyInterceptorServer::new(server))
    .serve(addr)
    .await.unwrap();
}
