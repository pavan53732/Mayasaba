//! Mayasaba validation service boundary.
pub const CRATE_NAME:&str="mayasaba-validation";

pub fn record_validation_run(storage:&mayasaba_storage::Storage,run:&mayasaba_storage::NewValidationRun)->mayasaba_storage::Result<()>{
 storage.insert_validation_run(run)
}

pub fn record_certification_binding(storage:&mayasaba_storage::Storage,binding:&mayasaba_storage::NewCertificationBinding)->mayasaba_storage::Result<()>{
 storage.insert_certification_binding(binding)
}
