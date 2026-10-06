//! Mayasaba evidence/artifact service boundary.
use std::{fs::File,io::{Read},path::Path};
use sha2::{Digest,Sha256};

pub const CRATE_NAME:&str="mayasaba-evidence";

#[derive(Debug)]
pub enum EvidenceError{Io(std::io::Error),Storage(mayasaba_storage::StorageError),InvalidArtifactPath(String)}
impl std::fmt::Display for EvidenceError{
 fn fmt(&self,f:&mut std::fmt::Formatter<'_>)->std::fmt::Result{
  match self{Self::Io(e)=>write!(f,"evidence I/O failed: {e}"),Self::Storage(e)=>write!(f,"evidence storage failed: {e}"),Self::InvalidArtifactPath(e)=>write!(f,"invalid artifact path: {e}") }
 }
}
impl std::error::Error for EvidenceError{}
impl From<std::io::Error> for EvidenceError{fn from(e:std::io::Error)->Self{Self::Io(e)}}
impl From<mayasaba_storage::StorageError> for EvidenceError{fn from(e:mayasaba_storage::StorageError)->Self{Self::Storage(e)}}

/// Hash a local file using bounded memory.
pub fn sha256_file(path:impl AsRef<Path>)->Result<(String,u64),EvidenceError>{
 let path=path.as_ref();
 let mut file=File::open(path)?;
 let size=file.metadata()?.len();
 let mut h=Sha256::new(); let mut buf=[0u8;65536];
 loop{let n=file.read(&mut buf)?; if n==0{break} h.update(&buf[..n]);}
 Ok((format!("{:x}",h.finalize()),size))
}

/// Capture one existing file as an immutable artifact plus FILE_HASH evidence.
/// WorkspaceService remains responsible for scope/authorization before calling this function.
pub fn capture_file(
 storage:&mayasaba_storage::Storage,
 artifact_id:&str,
 evidence_id:&str,
 project_id:&str,
 path:impl AsRef<Path>,
 created_at:&str
)->Result<mayasaba_storage::ArtifactRecord,EvidenceError>{
 let path=path.as_ref();
 if !path.is_file(){return Err(EvidenceError::InvalidArtifactPath(path.display().to_string()));}
 let (sha,size)=sha256_file(path)?;
 let artifact=storage.insert_artifact(&mayasaba_storage::NewArtifact{
  artifact_id:artifact_id.to_owned(),project_id:project_id.to_owned(),kind:"FILE".to_owned(),
  path:Some(path.to_string_lossy().into_owned()),sha256:Some(sha.clone()),
  size_bytes:Some(i64::try_from(size).map_err(|_|EvidenceError::InvalidArtifactPath("file too large".to_owned()))?),
  created_at:created_at.to_owned()
 })?;
 storage.insert_evidence(&mayasaba_storage::NewEvidence{
  evidence_id:evidence_id.to_owned(),project_id:project_id.to_owned(),kind:"FILE_HASH".to_owned(),
  source_json:serde_json::json!({"artifact_id":artifact_id,"path":path.to_string_lossy(),"sha256":sha,"size_bytes":size}).to_string(),
  sha256:artifact.sha256.clone(),created_at:created_at.to_owned()
 })?;
 storage.link_evidence_artifact(evidence_id,artifact_id)?;
 Ok(artifact)
}
