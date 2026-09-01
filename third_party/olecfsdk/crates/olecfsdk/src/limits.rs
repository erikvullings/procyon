#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
  pub max_file_size: u64,
  pub max_stream_size: u64,
  pub max_entries: usize,
  pub max_allocation: usize,
}

impl Default for Limits {
  fn default() -> Self {
    Self {
      max_file_size: 1 << 34,
      max_stream_size: 1 << 32,
      max_entries: 1_000_000,
      max_allocation: 1 << 30,
    }
  }
}
