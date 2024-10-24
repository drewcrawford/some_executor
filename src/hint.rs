/**
A type that describes the expected runtime characteristics of the future.
*/
#[non_exhaustive]
#[derive(Copy,Clone)]
pub enum Hint {
    /**
    We don't know anything about the future.
    */
    Unknown,
    /**
    The future is expected to spend most of its time yielded.
    */
    IO,

    /**
    The future is expected to spend most of its time computing.
    */
    CPU,
}

impl Default for Hint {
    fn default() -> Self {
        Hint::Unknown
    }
}