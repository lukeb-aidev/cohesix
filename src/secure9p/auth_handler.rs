// CLASSIFICATION: COMMUNITY
// Filename: auth_handler.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-24

#[derive(Clone)]
pub struct NullAuth;

pub trait AuthHandler {
    fn authenticate(&self, _hello: &[u8]) -> String;
}

impl AuthHandler for NullAuth {
    fn authenticate(&self, _hello: &[u8]) -> String {
        "anonymous".into()
    }
}
