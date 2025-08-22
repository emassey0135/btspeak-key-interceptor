use bitflags::{bitflags, Flags};
use evdev::{enumerate, EventSummary, KeyCode};
use evdev::uinput::VirtualDevice;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
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
#[derive(Debug)]
struct MyServer {
  state: Arc<Mutex<State>>,
  combination_rx: mpsc::Receiver<KeyFlags>,
  event_rx: mpsc::Receiver<(KeyFlags, bool)>,
}
#[tonic::async_trait]
impl BtspeakKeyInterceptor for MyServer {
  type GrabKeyCombinationsStream = ReceiverStream<Result<BrailleKeyCombination, Status>>;
  async fn grab_key_combinations(&self, request: Request<Empty>) -> Result<Response<Self::GrabKeyCombinationsStream>, Status> {
    let (tx, rx) = mpsc::channel(32);
    Ok(Response::new(ReceiverStream::new(rx)))
  }
  type GrabKeyEventsStream = ReceiverStream<Result<BrailleKeyEvent, Status>>;
  async fn grab_key_events(&self, request: Request<Empty>) -> Result<Response<Self::GrabKeyEventsStream>, Status> {
    let (tx, rx) = mpsc::channel(32);
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
  async fn release_keyboard(&self, request: Request<Empty>) -> Result<Response<Empty>, Status> {
    let mut state = self.state.lock().unwrap();
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
          if held_keys==KeyFlags::empty() {
            if state.lock().unwrap().sending_key_combinations && pressed_keys!=KeyFlags::empty() {
              combination_tx.send(pressed_keys.clone()).await.unwrap();
            };
            pressed_keys.clear();
          };
          if state.lock().unwrap().sending_key_events {
            let released = value==0;
            event_tx.send((flag.clone(), released)).await.unwrap();
          };
          if !state.lock().unwrap().sending_key_combinations && !state.lock().unwrap().sending_key_events {
            virtual_device.emit(&[event]).unwrap();
          };
        },
        _ => {}
      };
    };
  });
  let addr = "127.0.0.1:54123".parse().unwrap();
  let server = MyServer { state: state2, combination_rx, event_rx };
  Server::builder()
    .add_service(BtspeakKeyInterceptorServer::new(server))
    .serve(addr)
    .await.unwrap();
}
