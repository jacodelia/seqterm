pub mod bridge;
pub mod domain;
pub mod error;
pub mod migration;
pub mod ports;
pub mod registry;
pub mod stz;
pub mod validation;

pub use domain::{
    AssetEntry, AssetType, AutomationPoint, ChainRef, InterpolationMode, Manifest, MidiNote,
    PluginFormat, ProjectSnapshot, StzAudioClip, StzAutomationLane, StzBus, StzContainer,
    StzFxSlot, StzMidiClip, StzMixerChannel, StzNote, StzPattern, StzPatternSource,
    StzPluginInstance, StzProject, StzRoutingEdge, StzRoutingGraph, StzRoutingNode, StzSend,
    StzSyncMode, StzTempoMap, StzTimeline, StzTrack, StzTransport, TempoEvent,
    TimeSignatureEvent, TrackBlock, STZ_FORMAT, STZ_FORMAT_VERSION,
};
pub use error::{StzError, StzResult};
pub use migration::ProjectMigrator;
pub use ports::{
    ArchiveEntry, ArchivePort, AssetStoragePort, Migration, ProjectStoragePort,
    ProjectValidatorPort,
};
pub use registry::{AssetRegistry, ObjectRegistry};
pub use stz::{StzProjectStorage, incremental_save, make_asset_entry, sha256_hex};
pub use validation::DefaultValidator;
pub use bridge::{from_core, to_core};

/// Convenience: save a container to disk using the default ZIP-based storage adapter.
pub fn save(container: &StzContainer, path: &std::path::Path) -> StzResult<()> {
    use ports::ProjectStoragePort;
    StzProjectStorage.save(container, path)
}

/// Convenience: load a container from a `.stz` file.
pub fn load(path: &std::path::Path) -> StzResult<StzContainer> {
    use ports::ProjectStoragePort;
    StzProjectStorage.load(path)
}
