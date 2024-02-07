pub mod pb;
pub mod error;
pub mod storage;
pub mod service;
pub mod network;

pub use pb::abi::*;
pub use error::KvError;
pub use storage::*;
pub use service::*;
pub use network::*;

pub fn add(left: usize, right: usize) -> usize {
    left + right
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        let result = add(2, 2);
        assert_eq!(result, 4);
    }
}
