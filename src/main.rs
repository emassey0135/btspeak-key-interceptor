use evdev::{enumerate, EventSummary};
use evdev::uinput::VirtualDevice;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{transport::Server, Request, Response, Status};
use btspeak_key_interceptor::btspeak_key_interceptor_server::{BtspeakKeyInterceptor, BtspeakKeyInterceptorServer};
use btspeak_key_interceptor::{Empty, BrailleKeyCombination, BrailleKeyCombinations, BrailleKeyEvent, BrailleKeyEvents};
pub mod btspeak_key_interceptor {
  tonic::include_proto!("btspeak_key_interceptor");
}
#[derive(Debug, Default)]
pub struct MyServer {}
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
//  let (keycode_tx, mut keycode_rx) = mpsc::channel(32);
  tokio::spawn(async move {
    while let Ok(event) = event_stream.next_event().await {
      match event.destructure() {
        EventSummary::Key(_, code, value) => {
          println!("Key {:?}, value {}", code, value);
          virtual_device.emit(&[event]).unwrap();
        },
        _ => {}
      };
    };
  });
  let addr = "127.0.0.1:54123".parse().unwrap();
  let server = MyServer::default();
  Server::builder()
    .add_service(BtspeakKeyInterceptorServer::new(server))
    .serve(addr)
    .await.unwrap();
}
