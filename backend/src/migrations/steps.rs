//! What each data version does beyond adding, dropping or changing a column or a field: the steps
//! `MIGRATIONS` names, by the part of Janus they change. They exist only to bring old data to the
//! current shape, so the rest of Janus keeps only today's logic. A new version's step goes in the
//! file of what it changes.

pub(super) mod agents;
pub(super) mod library;
pub(super) mod roles;
pub(super) mod runs;
pub(super) mod settings;
