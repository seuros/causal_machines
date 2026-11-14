#![no_std]
#![doc = include_str!("../README.md")]

mod tracker;
mod vector;

pub use tracker::{Absorbing, Causal, Forgetting, Gap, Page, Readiness, Receipt, Release, Stamp};
pub use vector::{Causality, Membership, VersionVector};

/// Which replica an event came from.
pub type ReplicaId = u64;

/// Why a call was not honoured. A refused call leaves every state untouched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// The replica or slot is not part of the fixed membership.
    UnknownMember,
    /// The stamp was issued under an older membership generation.
    StaleGeneration,
    /// The stamp was issued under a membership generation not yet adopted here.
    FutureGeneration,
    /// The clock counts a slot outside the membership, or a stamp's sender
    /// entry is zero.
    Malformed,
    /// The counter is at `u64::MAX`; no further event can be numbered.
    Exhausted,
    /// A parked stamp from the same sender with the same counter carries a
    /// different clock. Answered whatever the readiness of either stamp:
    /// deliverable, missing dependencies or already covered.
    Conflict,
    /// Dependencies are missing and every hold slot is taken.
    Full,
}

impl core::fmt::Display for Refusal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Self::UnknownMember => "unknown member",
            Self::StaleGeneration => "stale membership generation",
            Self::FutureGeneration => "future membership generation",
            Self::Malformed => "malformed clock",
            Self::Exhausted => "counter exhausted",
            Self::Conflict => "conflicting stamp already held",
            Self::Full => "hold table full",
        })
    }
}

impl core::error::Error for Refusal {}

#[cfg(test)]
mod tests;
