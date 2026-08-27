use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

#[derive(Clone)]
struct Manifest {
    document_id: &'static str,
    snapshot_hash: &'static str,
    segment_hash: &'static str,
}

impl Manifest {
    fn canonical(&self) -> String {
        format!(
            "{{\"document_id\":\"{}\",\"segment_hash\":\"{}\",\"snapshot_hash\":\"{}\"}}",
            self.document_id, self.segment_hash, self.snapshot_hash
        )
    }
}

fn digest(input: &str) -> String {
    let mut hasher = DefaultHasher::new();
    input.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn sign(manifest: &Manifest, secret: &str) -> String {
    digest(&format!("{}:{secret}", manifest.canonical()))
}

fn verify(manifest: &Manifest, secret: &str, signature: &str) -> bool {
    sign(manifest, secret) == signature
}

fn main() {
    let secret = "local-development-secret";
    let manifest = Manifest {
        document_id: "doc_01",
        snapshot_hash: "sha256:snapshot",
        segment_hash: "sha256:segment",
    };
    let signature = sign(&manifest, secret);
    let mut tampered = manifest.clone();
    tampered.snapshot_hash = "sha256:changed";

    println!("canonical={}", manifest.canonical());
    println!("signature={signature}");
    println!("valid={}", verify(&manifest, secret, &signature));
    println!("tampered_valid={}", verify(&tampered, secret, &signature));
}
