// SPDX-License-Identifier: MIT OR Apache-2.0

/**
A type that describes the expected runtime characteristics of the task.
*/
#[non_exhaustive]
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Default)]
pub enum Hint {
    /**
    We don't know anything about the task.
    */
    #[default]
    Unknown,
    /**
    The task is expected to spend most of its time yielded.
    */
    IO,

    /**
    The task is expected to spend most of its time computing.
    */
    CPU,
}
