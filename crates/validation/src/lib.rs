//! Mayasaba validation crate boundary.
pub const CRATE_NAME: &str = "mayasaba-validation";

/// Persist a certification binding only after a real validation run exists. SQLite foreign keys enforce the
/// validation reference; the validation service remains the authority over whether certification is warranted.
pub fn record_certification_binding(storage: &mayasaba_storage::Storage, binding: &mayasaba_storage::NewCertificationBinding) -> mayasaba_storage::Result<()> {
    storage.insert_certification_binding(binding)
}
