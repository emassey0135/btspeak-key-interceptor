fn main() {
  tonic_prost_build::compile_protos("proto/btspeak_key_interceptor.proto").unwrap();
}
