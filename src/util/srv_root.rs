// CLASSIFICATION: COMMUNITY
// Filename: srv_root.rs v0.1
// Author: Lukas Bower
// Date Modified: 2026-10-28

#[macro_export]
macro_rules! with_srv_root {
    ($path:expr) => {{
        let root = std::env::var("COHESIX_SRV_ROOT").unwrap_or("/srv".to_string());
        format!(
            "{}/{}",
            root.trim_end_matches('/'),
            $path.trim_start_matches('/')
        )
    }};
}
