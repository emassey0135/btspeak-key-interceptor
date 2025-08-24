use bitflags::{bitflags, Flags};
use evdev::{enumerate, EventSummary, KeyCode};
use evdev::event_variants::KeyEvent;
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
  excluded_key_combinations: Vec<KeyFlags>,
  excluded_key_events: Vec<(KeyFlags, bool)>,
}
struct MyServer {
  state: Arc<Mutex<State>>,
  combination_rx: Arc<Mutex<mpsc::Receiver<KeyFlags>>>,
  combination_sender: Arc<Mutex<Cell<Option<(CancellationToken, oneshot::Receiver<()>)>>>>,
  event_rx: Arc<Mutex<mpsc::Receiver<(KeyFlags, bool)>>>,
  event_sender: Arc<Mutex<Cell<Option<(CancellationToken, oneshot::Receiver<()>)>>>>,
  combination_tx: mpsc::Sender<KeyFlags>,
  event_tx: mpsc::Sender<(KeyFlags, bool)>,
}
#[tonic::async_trait]
impl BtspeakKeyInterceptor for MyServer {
  type GrabKeyCombinationsStream = ReceiverStream<Result<BrailleKeyCombination, Status>>;
  async fn grab_key_combinations(&self, _request: Request<Empty>) -> Result<Response<Self::GrabKeyCombinationsStream>, Status> {
    let combination_sender = self.combination_sender.lock().await;
    self.state.lock().await.sending_key_combinations = true;
    let (tx, rx) = mpsc::channel(32);
    let combination_rx = self.combination_rx.clone();
    let canceller = CancellationToken::new();
    let (cancel_tx, cancel_rx) = oneshot::channel();
    combination_sender.set(Some((canceller.clone(), cancel_rx)));
    let state = self.state.clone();
    let combination_sender = self.combination_sender.clone();
    tokio::spawn(async move {
      loop {
        let mut combination_rx = combination_rx.lock().await;
        select! {
          _ = canceller.cancelled() => {
            state.lock().await.excluded_key_combinations = vec!();
            cancel_tx.send(()).unwrap();
            break;
          },
          result = combination_rx.recv() => {
            match result {
              Some(combination) => {
                let dots = (combination.clone() - KeyFlags::Space).bits();
                let space = combination.contains(KeyFlags::Space);
                let combination = BrailleKeyCombination { dots: dots as i32, space };
                match tx.send(Ok(combination)).await {
                  Ok(_) => {},
                  _ => {
                    let mut state = state.lock().await;
                    state.sending_key_combinations = false;
                    state.excluded_key_combinations = vec!();
                    combination_sender.lock().await.set(None);
                    break;
                  },
                };
              },
              None => {
                let mut state = state.lock().await;
                state.sending_key_combinations = false;
                state.excluded_key_combinations = vec!();
                combination_sender.lock().await.set(None);
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
  async fn grab_key_events(&self, _request: Request<Empty>) -> Result<Response<Self::GrabKeyEventsStream>, Status> {
    let event_sender = self.event_sender.lock().await;
    self.state.lock().await.sending_key_events = true;
    let (tx, rx) = mpsc::channel(32);
    let event_rx = self.event_rx.clone();
    let canceller = CancellationToken::new();
    let (cancel_tx, cancel_rx) = oneshot::channel();
    event_sender.set(Some((canceller.clone(), cancel_rx)));
    let event_sender = self.event_sender.clone();
    let state = self.state.clone();
    tokio::spawn(async move {
      loop {
        let mut event_rx = event_rx.lock().await;
        select! {
          _ = canceller.cancelled() => {
            state.lock().await.excluded_key_events = vec!();
            cancel_tx.send(()).unwrap();
            break;
          },
          result = event_rx.recv() => {
            match result {
              Some(event) => {
                let dot = event.0.bits();
                let release = event.1;
                let event = BrailleKeyEvent { dot: dot as i32, release };
                match tx.send(Ok(event)).await {
                  Ok(_) => {},
                  _ => {
                    let mut state = state.lock().await;
                    state.sending_key_events = false;
                    state.excluded_key_events = vec!();
                    event_sender.lock().await.set(None);
                    break;
                  },
                };
              },
              None => {
                let mut state = state.lock().await;
                state.sending_key_events = false;
                state.excluded_key_events = vec!();
                event_sender.lock().await.set(None);
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
    let mut state = self.state.lock().await;
    if !state.sending_key_combinations {
      return Err(Status::failed_precondition("Not currently sending key combinations"));
    };
    let combinations = request
      .into_inner()
      .combinations
      .into_iter()
      .map(|combination| {
        let dots = KeyFlags::from_bits_truncate(combination.dots as u16);
        if combination.space {
          dots | KeyFlags::Space
        }
        else {
          dots
        }
      })
      .collect::<Vec<KeyFlags>>();
    state.excluded_key_combinations = combinations;
    let reply = Empty {};
    Ok(Response::new(reply))
  }
  async fn set_excluded_key_events(&self, request: Request<BrailleKeyEvents>) -> Result<Response<Empty>, Status> {
    let mut state = self.state.lock().await;
    if !state.sending_key_events {
      return Err(Status::failed_precondition("Not currently sending key events"));
    };
    let events = request
      .into_inner()
      .events
      .into_iter()
      .map(|event| {
        let dot = KeyFlags::from_bits_truncate(event.dot as u16);
        (dot, event.release)
      })
      .collect::<Vec<(KeyFlags, bool)>>();
    state.excluded_key_events = events;
    let reply = Empty {};
    Ok(Response::new(reply))
  }
  async fn release_keyboard(&self, _request: Request<Empty>) -> Result<Response<Empty>, Status> {
    {
      let mut state = self.state.lock().await;
      state.sending_key_combinations = false;
      state.sending_key_events = false;
    };
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
    let reply = Empty {};
    Ok(Response::new(reply))
  }
  async fn send_key_combination(&self, request: Request<BrailleKeyCombination>) -> Result<Response<Empty>, Status> {
    let combination = request.into_inner();
    let dots = KeyFlags::from_bits_truncate(combination.dots as u16);
    let combination = if combination.space {
      dots | KeyFlags::Space
    }
    else {
      dots
    };
    self.combination_tx.send(combination).await.unwrap();
    let reply = Empty {};
    Ok(Response::new(reply))
  }
  async fn send_key_event(&self, request: Request<BrailleKeyEvent>) -> Result<Response<Empty>, Status> {
    let event = request.into_inner();
    let key = KeyFlags::from_bits_truncate(event.dot as u16);
    let event = (key, event.release);
    self.event_tx.send(event).await.unwrap();
    let reply = Empty {};
    Ok(Response::new(reply))
  }
}
fn key_flag_to_key_code(flag: KeyFlags) -> KeyCode {
  match flag {
    KeyFlags::Dot1 => KeyCode::KEY_BRL_DOT1,
    KeyFlags::Dot2 => KeyCode::KEY_BRL_DOT2,
    KeyFlags::Dot3 => KeyCode::KEY_BRL_DOT3,
    KeyFlags::Dot4 => KeyCode::KEY_BRL_DOT4,
    KeyFlags::Dot5 => KeyCode::KEY_BRL_DOT5,
    KeyFlags::Dot6 => KeyCode::KEY_BRL_DOT6,
    KeyFlags::Dot7 => KeyCode::KEY_BRL_DOT7,
    KeyFlags::Dot8 => KeyCode::KEY_BRL_DOT8,
    KeyFlags::Space => KeyCode::KEY_SPACE,
    _ => panic!("Invalid key event"),
  }
}
#[tokio::main]
async fn main() {
  let mut device = enumerate()
    .find(|(_, device)| device.name().map_or(false, |name| name=="4x3braille"))
    .unwrap()
    .1;
  device.grab().unwrap();
  let virtual_device = VirtualDevice::builder()
    .unwrap()
    .name("btspeak-key-interceptor")
    .with_keys(device.supported_keys().unwrap())
    .unwrap()
    .build()
    .unwrap();
  let virtual_device = Arc::new(Mutex::new(virtual_device));
  let virtual_device2 = virtual_device.clone();
  let mut event_stream = device.into_event_stream().unwrap();
  let state = Arc::new(Mutex::new(State {
    sending_key_combinations: false,
    sending_key_events: false,
    excluded_key_combinations: vec!(),
    excluded_key_events: vec!(),
  }));
  let state2 = state.clone();
  let (combination_tx, combination_rx) = mpsc::channel(32);
  let (event_tx, event_rx) = mpsc::channel(32);
  let (combination_tx2, mut combination_rx2) = mpsc::channel::<KeyFlags>(32);
  let (event_tx2, mut event_rx2) = mpsc::channel::<(KeyFlags, bool)>(32);
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
          let state = state.lock().await;
          if state.sending_key_events && state.excluded_key_events.contains(&(flag.clone(), value==0)) {
            virtual_device.lock().await.emit(&[event]).unwrap();
          }
          else {
            match value {
              0 => held_keys -= flag.clone(),
              _ => {
                pressed_keys |= flag.clone();
                held_keys |= flag.clone();
              },
            };
            if held_keys==KeyFlags::empty() {
              if state.sending_key_combinations && pressed_keys!=KeyFlags::empty() {
                if state.excluded_key_combinations.contains(&pressed_keys) {
                  let codes = pressed_keys
                    .iter()
                    .map(|flag| key_flag_to_key_code(flag))
                    .collect::<Vec<KeyCode>>();
                  let mut virtual_device = virtual_device.lock().await;
                  for code in codes.clone() {
                    virtual_device.emit(&[KeyEvent::new(code, 1).into()]).unwrap();
                  };
                  for code in codes {
                    virtual_device.emit(&[KeyEvent::new(code, 0).into()]).unwrap();
                  };
                }
                else {
                  combination_tx.send(pressed_keys.clone()).await.unwrap();
                };
              };
              pressed_keys.clear();
            };
            if state.sending_key_events {
              let released = value==0;
              event_tx.send((flag.clone(), released)).await.unwrap();
            };
            if !state.sending_key_combinations && !state.sending_key_events {
              virtual_device.lock().await.emit(&[event]).unwrap();
            };
          };
        },
        _ => {}
      };
    };
  });
  tokio::spawn(async move {
    loop {
      select! {
        result = combination_rx2.recv() => {
          if let Some(combination) = result {
            let codes = combination
              .iter()
              .map(|flag| key_flag_to_key_code(flag))
              .collect::<Vec<KeyCode>>();
            let mut virtual_device2 = virtual_device2.lock().await;
            for code in codes.clone() {
              virtual_device2.emit(&[KeyEvent::new(code, 1).into()]).unwrap();
            };
            for code in codes {
              virtual_device2.emit(&[KeyEvent::new(code, 0).into()]).unwrap();
            };
          }
          else {
            break;
          };
        },
        result = event_rx2.recv() => {
          if let Some(event) = result {
            let code = key_flag_to_key_code(event.0);
            let value = if event.1 {
              0
            }
            else {
              1
            };
            virtual_device2.lock().await.emit(&[KeyEvent::new(code, value).into()]).unwrap();
          }
          else {
            break;
          };
        },
      };
    };
  });
  let addr = "127.0.0.1:54123".parse().unwrap();
  let server = MyServer {
    state: state2,
    combination_rx: Arc::new(Mutex::new(combination_rx)),
    combination_sender: Arc::new(Mutex::new(Cell::new(None))),
    event_rx: Arc::new(Mutex::new(event_rx)),
    event_sender: Arc::new(Mutex::new(Cell::new(None))),
    combination_tx: combination_tx2,
    event_tx: event_tx2,
  };
  Server::builder()
    .add_service(BtspeakKeyInterceptorServer::new(server))
    .serve(addr)
    .await.unwrap();
}
