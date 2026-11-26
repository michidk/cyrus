//! Public native control contracts, grouped by command responsibility.
mod fixtures;
use super::*;
use cyrus_assets::surface::Surface;
use cyrus_control::Request;
use fixtures::*;
use std::sync::mpsc::channel;

mod acquisition;
mod capture;
mod inspection;
mod rendering;
mod replay;
mod script_audio;
