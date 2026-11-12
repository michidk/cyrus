//! Installed emitter listener acceptance using the production rendered camera transform.
#[path = "../../../dev/fixtures/listener-publication.rs"]
mod probe;
fn main() {
    probe::run(cyrus_render::camera::driven_view_transform);
}
