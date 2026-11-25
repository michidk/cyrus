//! Integration-fixture access, excluded unless `test-support` is enabled.
//! Ordinary hosts must use gameplay stages, property dispatch and snapshot admission.
pub use super::activation::test_support::GameplayGlobalsTestExt;
pub use super::registry::test_support::ScriptWorldTestExt;
