use evdev::{enumerate, EventSummary};
use evdev::uinput::VirtualDevice;
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
  loop {
    while let Ok(event) = event_stream.next_event().await {
      match event.destructure() {
        EventSummary::Key(_, code, value) => {
          println!("Key {:?}, value {}", code, value);
          virtual_device.emit(&[event]).unwrap();
        },
        _ => {}
      };
    };
  };
}
