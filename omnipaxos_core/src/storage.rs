use super::ballot_leader_election::Ballot;
use std::{fmt::Debug, marker::PhantomData};
/// Type of the entries stored in the log.
pub trait Entry: Clone + Debug {}

impl<T> Entry for T where T: Clone + Debug {}

/// A StopSign entry that marks the end of a configuration. Used for reconfiguration.
#[derive(Clone, Debug)]
#[allow(missing_docs)]
pub struct StopSignEntry {
    pub stopsign: StopSign,
    pub decided: bool,
}

impl StopSignEntry {
    /// Creates a [`StopSign`].
    pub fn with(stopsign: StopSign, decided: bool) -> Self {
        StopSignEntry { stopsign, decided }
    }
}

/// A StopSign entry that marks the end of a configuration. Used for reconfiguration.
#[derive(Clone, Debug)]
pub struct StopSign {
    /// The identifier for the new configuration.
    pub config_id: u32,
    /// The process ids of the new configuration.
    pub nodes: Vec<u64>,
    /// Metadata for the reconfiguration. Can be used for pre-electing leader for the new configuration and skip prepare phase when starting the new configuration with the given leader.
    pub metadata: Option<Vec<u8>>,
}

impl StopSign {
    /// Creates a [`StopSign`].
    pub fn with(config_id: u32, nodes: Vec<u64>, metadata: Option<Vec<u8>>) -> Self {
        StopSign {
            config_id,
            nodes,
            metadata,
        }
    }
}

impl PartialEq for StopSign {
    fn eq(&self, other: &Self) -> bool {
        self.config_id == other.config_id && self.nodes == other.nodes
    }
}

/// Snapshot type. A `Complete` snapshot contains all snapshotted data while `Delta` has snapshotted changes since an earlier snapshot.
#[allow(missing_docs)]
#[derive(Clone, Debug)]
pub enum SnapshotType<T, S>
where
    T: Entry,
    S: Snapshot<T>,
{
    Complete(S),
    Delta(S),
    _Phantom(PhantomData<T>),
}

/// Functions required by Sequence Paxos to implement snapshot operations for `T`. If snapshot is not desired to be used, use the unit type `()` as the Snapshot parameter in `SequencePaxos`.
pub trait Snapshot<T>: Clone
where
    T: Entry,
{
    /// Create a snapshot from the log `entries`.
    fn create(entries: &[T]) -> Self;

    /// Merge another snapshot `delta` into self.
    fn merge(&mut self, delta: Self);

    /// Whether `T` is snapshottable. If not, simply return `false` and leave the other functions `unimplemented!()`.
    fn use_snapshots() -> bool;

    //fn size_hint() -> u64;  // TODO: To let the system know trade-off of using entries vs snapshot?
}

/// CachedState is an in-memory state storage for SequencePaxos, the stuct
/// caches any new state that is written to persistent storage and
/// can be used recover state when an atomic commit fails.
#[derive(Clone)]
pub(crate) struct CachedState {
    /// Last cached promised round.
    n_prom: Ballot,
    /// Last cached accepted round.
    acc_round: Ballot,
    /// Length of the cached decided log.
    ld: u64,
    /// Garbage collected index.
    trimmed_idx: u64,
}

impl CachedState {
    pub fn set_promise(&mut self, n_prom: Ballot) {
        self.n_prom = n_prom;
    }

    pub fn set_decided_idx(&mut self, ld: u64) {
        self.ld = ld;
    }

    pub fn get_decided_idx(&self) -> u64 {
        self.ld
    }

    pub fn set_accepted_round(&mut self, na: Ballot) {
        self.acc_round = na;
    }

    pub fn get_accepted_round(&self) -> Ballot {
        self.acc_round
    }

    pub fn get_promise(&self) -> Ballot {
        self.n_prom
    }

    pub fn set_compacted_idx(&mut self, trimmed_idx: u64) {
        self.trimmed_idx = trimmed_idx;
    }

    pub fn get_compacted_idx(&self) -> u64 {
        self.trimmed_idx
    }
}

impl Default for CachedState {
    fn default() -> Self {
        Self {
            n_prom: Ballot::default(),
            acc_round: Ballot::default(),
            ld: 0,
            trimmed_idx: 0,
        }
    }
}

/// Trait for implementing the storage backend of Sequence Paxos.
pub trait Storage<T, S>
where
    T: Entry,
    S: Snapshot<T>,
{
    /// Appends an entry to the end of the log and returns the log length.
    fn append_entry(&mut self, entry: T) -> Result<u64, StorageErr>;

    /// Appends the entries of `entries` to the end of the log and returns the log length.
    fn append_entries(&mut self, entries: Vec<T>) -> Result<u64, StorageErr>;

    /// Appends the entries of `entries` to the prefix from index `from_index` in the log and returns the log length.
    fn append_on_prefix(&mut self, from_idx: u64, entries: Vec<T>) -> Result<u64, StorageErr>;

    /// Sets the round that has been promised.
    fn set_promise(&mut self, n_prom: Ballot) -> Result<(), StorageErr>;

    /// Sets the decided index in the log.
    fn set_decided_idx(&mut self, ld: u64) -> Result<(), StorageErr>;

    /// Returns the decided index in the log.
    fn get_decided_idx(&self) -> Result<u64, StorageErr>;

    /// Sets the latest accepted round.
    fn set_accepted_round(&mut self, na: Ballot) -> Result<(), StorageErr>;

    /// Returns the latest round in which entries have been accepted.
    fn get_accepted_round(&self) -> Result<Ballot, StorageErr>;

    /// Returns the entries in the log in the index interval of [from, to).
    /// If entries **do not exist for the complete interval**, an empty Vector should be returned.
    fn get_entries(&self, from: u64, to: u64) -> Result<Vec<T>, StorageErr>;

    /// Returns the current length of the log.
    fn get_log_len(&self) -> Result<u64, StorageErr>;

    /// Returns the suffix of entries in the log from index `from`.
    fn get_suffix(&self, from: u64) -> Result<Vec<T>, StorageErr>;

    /// Returns the round that has been promised.
    fn get_promise(&self) -> Result<Ballot, StorageErr>;

    /// Sets the StopSign used for reconfiguration.
    fn set_stopsign(&mut self, s: StopSignEntry) -> Result<(), StorageErr>;

    /// Returns the stored StopSign.
    fn get_stopsign(&self) -> Result<Option<StopSignEntry>, StorageErr>;

    /// Removes elements up to the given [`idx`] from storage.
    fn trim(&mut self, idx: u64) -> Result<(), StorageErr>;

    /// Sets the compacted (i.e. trimmed or snapshotted) index.
    fn set_compacted_idx(&mut self, idx: u64) -> Result<(), StorageErr>;

    /// Returns the garbage collector index from storage.
    fn get_compacted_idx(&self) -> Result<u64, StorageErr>;

    /// Sets the snapshot.
    fn set_snapshot(&mut self, snapshot: S) -> Result<(), StorageErr>;

    /// Returns the stored snapshot.
    fn get_snapshot(&self) -> Result<Option<S>, StorageErr>;
}

#[derive(Clone, Debug)]
#[allow(missing_docs)]
/// An error that describes the type of failure when writing to storage
pub enum StorageErr {
    LogError,
    StateError,
}

#[allow(missing_docs)]
impl<T: Entry> Snapshot<T> for () {
    fn create(_: &[T]) -> Self {
        unimplemented!()
    }

    fn merge(&mut self, _: Self) {
        unimplemented!()
    }

    fn use_snapshots() -> bool {
        false
    }
}
