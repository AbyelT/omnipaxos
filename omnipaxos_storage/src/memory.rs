
/// An persistent storage implementation, lets sequence paxos write the log
/// and variables (e.g. promised, accepted) to disk. Log entries are serialized 
/// and de-serialized into slice of bytes when read or written from the log. 
#[allow(missing_docs)]
pub mod persistent_storage {
    use std::marker::PhantomData;
    use omnipaxos_core::{
        ballot_leader_election::Ballot,
        storage::{Entry, Snapshot, StopSignEntry, Storage},
    };
    use commitlog::{
        message::{MessageSet, MessageBuf}, CommitLog, LogOptions, ReadLimit,
    };
    use rocksdb::{Options, DB};
    use zerocopy::{AsBytes, FromBytes};
    use std::mem::size_of;
    const COMMITLOG: &str = "commitlog/";
    const ROCKSDB: &str = "rocksDB/";
    //#[derive(Debug)]
    pub struct PersistentState<T, S>
    where
        T: Entry,
        S: Snapshot<T>,
    {
        /// a disk-based commit log for entries
        c_log: CommitLog,
        /// Todo: Path to commitlog, remove when commitlog is no longer deleted here
        c_log_path: String,
        /// a struct for accessing local RocksDB database
        db: DB,
        /// Garbage collected index.
        trimmed_idx: u64,
        /// Stored snapshot
        snapshot: Option<S>,
        /// Stored StopSign
        stopsign: Option<StopSignEntry>,
        /// A placeholder for the T: Entry
        marker: PhantomData<T>
    }

    impl<T: Entry, S: Snapshot<T>> PersistentState<T, S> {
        pub fn with(replica_id: &str) -> Self {

            // Paths to commitlog and rocksDB store
            let c_path: String = COMMITLOG.to_string() + &replica_id.to_string();
            let db_path = ROCKSDB.to_string() + &replica_id.to_string();

            // todo: a temporary solution, makes sure tests start with empty db and log, move later to tests!
            let _ = std::fs::remove_dir_all(&c_path);
            let _ = std::fs::remove_dir_all(&db_path);

            // Initialize a commitlog for entries
            let c_opts = LogOptions::new(&c_path);
            let c_log = CommitLog::new(c_opts).unwrap();

            // rocksDB
            let mut db_opts = Options::default();
            db_opts.increase_parallelism(4);                        // Set the amount threads for rocksDB compaction and flushing
            db_opts.create_if_missing(true);                        // Creates an database if its missing in the path
            let db = DB::open(&db_opts, &db_path).unwrap();
            Self {
                c_log: c_log,
                c_log_path: c_path,
                db: db,
                trimmed_idx: 0,
                snapshot: None,
                stopsign: None,
                marker: PhantomData::default()
            }
        }
    }

    impl<T, S> Storage<T, S> for PersistentState<T, S>
    where
        T: Entry + zerocopy::AsBytes + zerocopy::FromBytes,
        S: Snapshot<T>,
    {

        fn append_entry(&mut self, entry: T) -> u64 {
            let bytes = size_of::<T>();
            println!("append entry, bytes {:?}", bytes);
            let entry_bytes = AsBytes::as_bytes(&entry);
            match self.c_log.append_msg(entry_bytes) {
                Ok(x) => {
                    x
                },
                Err(_e) => 0,  
            }
        }

        fn append_entries(&mut self, entries: Vec<T>) -> u64 {
            //println!("append entries!");
            let mut buf: MessageBuf = MessageBuf::default();
            for entry in entries {
                let _ = buf.push(AsBytes::as_bytes(&entry));
            }
            self.c_log.append(&mut buf).unwrap();
            self.get_log_len()                       
        }

        fn append_on_prefix(&mut self, from_idx: u64, entries: Vec<T>) -> u64 {
            //println!("append on prefix!"); 
            let _ = self.c_log.truncate(from_idx);                    // truncate removes entries excluding 'from_idx' so subtract by 1
            self.append_entries(entries)
        }

        fn get_entries(&self, from: u64, to: u64) -> Vec<T> {
            //println!("get_entries from: {:?} -> to: {:?} ", from, to);
            let buffer = self.c_log.read(from, ReadLimit::default()).unwrap(); // todo: 32 is the magic number
            let mut entries = vec![];
            for (idx, msg) in buffer.iter().enumerate() {
                if (idx as u64 + from) >= to { break }                                                  // todo: find a clener solution                                               // check that the amount entres are equal 'to'
                entries.push(FromBytes::read_from(msg.payload()).unwrap());
            }
            //println!("res from get_entries {:?}", entries);
            entries   
        }

        fn get_log_len(&self) -> u64 {
            self.c_log.next_offset()
            //println!("log length: {:?}", res);
            //res
        }

        fn get_suffix(&self, from: u64) -> Vec<T> {
            //println!("get suffix!");
            self.get_entries(from, self.get_log_len())
        }

        fn get_promise(&self) -> Ballot {
            match self.db.get(b"n_prom") {
                Ok(Some(mut value)) => {
                    let prom_bytes: &mut [u8] = &mut value;
                    FromBytes::read_from(prom_bytes).unwrap()
                }
                Ok(None) => Ballot::default(), 
                Err(_e) => Ballot::default(),
            }
        }

        fn set_promise(&mut self, n_prom: Ballot) {
            let prom_bytes = AsBytes::as_bytes(&n_prom);
            self.db.put(b"n_prom", prom_bytes).unwrap()
        }

        fn get_decided_idx(&self) -> u64 {
            match self.db.get(b"ld") {
                Ok(Some(value)) => {
                    let ld_bytes: &[u8] = &value;
                    FromBytes::read_from(ld_bytes).unwrap()
                }
                Ok(None) => 0,
                Err(_e) => todo!(),
            }
        }

        fn set_decided_idx(&mut self, ld: u64) {
            let ld_bytes = AsBytes::as_bytes(&ld);
            self.db.put(b"ld", ld_bytes).unwrap();
        }

        fn get_accepted_round(&self) -> Ballot {
            match self.db.get(b"acc_round") {
                Ok(Some(mut value)) => {
                    let acc_bytes: &mut [u8] = &mut value;
                    FromBytes::read_from(acc_bytes).unwrap()
                }
                Ok(None) => Ballot::default(), 
                Err(_e) => Ballot::default(),
            }
        }

        fn set_accepted_round(&mut self, na: Ballot) {
            let acc_bytes = AsBytes::as_bytes(&na);
            self.db.put(b"n_prom", acc_bytes).unwrap();
        }

        fn set_stopsign(&mut self, s: StopSignEntry) {
            self.stopsign = Some(s);
        }

        fn get_stopsign(&self) -> Option<StopSignEntry> {
            self.stopsign.clone()
        }

        fn trim(&mut self, trimmed_idx: u64) {
            //println!("TRIM!");
            let mut trimmed_log: Vec<T> = self.get_entries(0, self.c_log.next_offset());    // get the entire log, drain it until trimmed_idx

            // println!("length before truncate {:?}", self.c_log.next_offset());
            // println!("the trimmed log before drain {:?}", trimmed_log);
            trimmed_log.drain(0..trimmed_idx as usize);
            // println!("the trimmed log after drain {:?}", trimmed_log);

            let _ = std::fs::remove_dir_all(&self.c_log_path);                              // remove old log

            let c_opts = LogOptions::new(&self.c_log_path);             // create new, insert the log into it
            self.c_log = CommitLog::new(c_opts).unwrap();
            self.append_entries(trimmed_log);
            //println!("length after truncate {:?}", self.get_log_len());    
        }

        fn set_compacted_idx(&mut self, trimmed_idx: u64) {
            //println!("set_compacted_idx!, {}", trimmed_idx);
            self.trimmed_idx = trimmed_idx;
        }

        fn get_compacted_idx(&self) -> u64 {
            //println!("get_compacted_idx!, {}", self.trimmed_idx);
            self.trimmed_idx
        }

        fn set_snapshot(&mut self, snapshot: S) {
            self.snapshot = Some(snapshot);
        }

        fn get_snapshot(&self) -> Option<S> {
            self.snapshot.clone()
        }
    }
}

pub mod memory_storage {
    use omnipaxos_core::{
        ballot_leader_election::Ballot,
        storage::{Entry, Snapshot, StopSignEntry, Storage},
    };
    /// An in-memory storage implementation for SequencePaxos.
    #[derive(Clone)]
    pub struct MemoryStorage<T, S>
    where
        T: Entry,
        S: Snapshot<T>,
    {
        /// Vector which contains all the logged entries in-memory.
        log: Vec<T>,
        /// Last promised round.
        n_prom: Ballot,
        /// Last accepted round.
        acc_round: Ballot,
        /// Length of the decided log.
        ld: u64,
        /// Garbage collected index.
        trimmed_idx: u64,
        /// Stored snapshot
        snapshot: Option<S>,
        /// Stored StopSign
        stopsign: Option<StopSignEntry>,
    }

    impl<T, S> Storage<T, S> for MemoryStorage<T, S>
    where
        T: Entry,
        S: Snapshot<T>,
    {
        fn append_entry(&mut self, entry: T) -> u64 {
            self.log.push(entry);
            self.get_log_len()
        }

        fn append_entries(&mut self, entries: Vec<T>) -> u64 {
            let mut e = entries;
            self.log.append(&mut e);
            self.get_log_len()
        }

        fn append_on_prefix(&mut self, from_idx: u64, entries: Vec<T>) -> u64 {
            self.log.truncate(from_idx as usize);
            self.append_entries(entries)
        }

        fn set_promise(&mut self, n_prom: Ballot) {
            self.n_prom = n_prom;
        }

        fn set_decided_idx(&mut self, ld: u64) {
            self.ld = ld;
        }

        fn get_decided_idx(&self) -> u64 {
            self.ld
        }

        fn set_accepted_round(&mut self, na: Ballot) {
            self.acc_round = na;
        }

        fn get_accepted_round(&self) -> Ballot {
            self.acc_round
        }

        fn get_entries(&self, from: u64, to: u64) -> Vec<T> {
            self.log.get(from as usize..to as usize).unwrap_or(&[]).to_vec() // todo added to_vec 
        }

        fn get_log_len(&self) -> u64 {
            self.log.len() as u64
        }

        fn get_suffix(&self, from: u64) -> Vec<T> {
            match self.log.get(from as usize..) {
                Some(s) => s.to_vec(),
                None => vec![],
            }
        }

        fn get_promise(&self) -> Ballot {
            self.n_prom
        }

        fn set_stopsign(&mut self, s: StopSignEntry) {
            self.stopsign = Some(s);
        }

        fn get_stopsign(&self) -> Option<StopSignEntry> {
            self.stopsign.clone()
        }

        fn trim(&mut self, trimmed_idx: u64) {
            self.log.drain(0..trimmed_idx as usize);
        }

        fn set_compacted_idx(&mut self, trimmed_idx: u64) {
            self.trimmed_idx = trimmed_idx;
        }

        fn get_compacted_idx(&self) -> u64 {
            self.trimmed_idx
        }

        fn set_snapshot(&mut self, snapshot: S) {
            self.snapshot = Some(snapshot);
        }

        fn get_snapshot(&self) -> Option<S> {
            self.snapshot.clone()
        }
    }

    impl<T: Entry, S: Snapshot<T>> Default for MemoryStorage<T, S> {
        fn default() -> Self {
            Self {
                log: vec![],
                n_prom: Ballot::default(),
                acc_round: Ballot::default(),
                ld: 0,
                trimmed_idx: 0,
                snapshot: None,
                stopsign: None,
            }
        }
    }
}
