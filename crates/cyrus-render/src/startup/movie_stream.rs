//! Browser fetch backpressure and streamed WAV audio retain bounded movie preparation.
use bevy::platform::time::Instant;
use cyrus_assets::movie::{Prepared, stream::MovieStream};
use std::collections::VecDeque;
use wasm_bindgen::prelude::*;

#[wasm_bindgen(module = "/src/startup/movie_stream.js")]
extern "C" {
    #[wasm_bindgen(catch, js_name = openMovie)]
    fn open_movie(
        url: &str,
        audio: bool,
        duration: f64,
        frame_seconds: f64,
    ) -> Result<u32, JsValue>;
    #[wasm_bindgen(js_name = movieChunk)]
    fn movie_chunk(id: u32) -> JsValue;
    #[wasm_bindgen(js_name = movieError)]
    fn movie_error(id: u32) -> Option<String>;
    #[wasm_bindgen(js_name = movieComplete)]
    fn movie_complete(id: u32) -> bool;
    #[wasm_bindgen(js_name = movieReady)]
    fn movie_ready(id: u32) -> bool;
    #[wasm_bindgen(js_name = movieVideoEnd)]
    fn movie_video_end(id: u32, end: f64);
    #[wasm_bindgen(js_name = movieTime)]
    fn movie_time(id: u32) -> f64;
    #[wasm_bindgen(js_name = playMovie)]
    fn play_movie(id: u32);
    #[wasm_bindgen(catch, js_name = seekMovie)]
    fn seek_movie(id: u32, seconds: f64) -> Result<(), JsValue>;
    #[wasm_bindgen(js_name = closeMovie)]
    fn close_movie(id: u32);
}

pub(super) struct BrowserMovie {
    id: u32,
    stream: MovieStream,
    frames: VecDeque<(u32, Vec<u8>)>,
    target: u32,
    total: u32,
    frame_seconds: f64,
    name: String,
}
impl BrowserMovie {
    pub fn new(movie: &Prepared) -> anyhow::Result<Self> {
        let frame_seconds = f64::from(movie.metadata.frame_micros) / 1_000_000.0;
        let id = open_movie(
            &movie.video_url,
            movie.metadata.has_audio,
            f64::from(movie.metadata.frames) * frame_seconds,
            frame_seconds,
        )
        .map_err(|error| anyhow::anyhow!("movie {}: {error:?}", movie.metadata.definition.name))?;
        Ok(Self {
            id,
            stream: MovieStream::new(movie.metadata.clone()),
            frames: VecDeque::new(),
            target: 1,
            total: movie.metadata.frames,
            frame_seconds: f64::from(movie.metadata.frame_micros) / 1_000_000.0,
            name: movie.metadata.definition.name.clone(),
        })
    }
    pub fn pump(&mut self, elapsed: f64) -> anyhow::Result<()> {
        if let Some(error) = movie_error(self.id) {
            anyhow::bail!("movie {}: {error}", self.name);
        }
        let ahead = (((elapsed + 0.5) / self.frame_seconds).floor() as u32)
            .saturating_add(1)
            .max(self.target)
            .min(self.total);
        let start = Instant::now();
        while self.stream.decoded_frames() < ahead
            && start.elapsed() < super::movie::BROWSER_DECODE_BUDGET
        {
            if let Some(frame) = self.stream.next_frame()? {
                if frame.number >= self.target {
                    self.frames.push_back((
                        frame.number,
                        super::movie::stretch(frame.indices, frame.palette),
                    ));
                }
            } else {
                let chunk = movie_chunk(self.id);
                if chunk.is_null() || chunk.is_undefined() {
                    anyhow::ensure!(
                        !movie_complete(self.id),
                        "movie {}: truncated frame {}",
                        self.name,
                        self.stream.decoded_frames() + 1
                    );
                    break;
                }
                self.stream.push(&js_sys::Uint8Array::new(&chunk).to_vec());
            }
        }
        if self.stream.decoded_frames() >= self.target {
            movie_video_end(
                self.id,
                f64::from(self.stream.decoded_frames()) * self.frame_seconds,
            );
        }
        Ok(())
    }
    pub fn next(&mut self) -> Option<Vec<u8>> {
        if !movie_ready(self.id) {
            return None;
        }
        self.frames.pop_front().map(|(_, pixels)| pixels)
    }
    pub fn seek(&mut self, frame: u32) -> anyhow::Result<()> {
        self.target = frame;
        self.frames.retain(|(number, _)| *number >= frame);
        seek_movie(self.id, f64::from(frame - 1) * self.frame_seconds)
            .map_err(|error| anyhow::anyhow!("movie {}: audio seek: {error:?}", self.name))
    }
    pub fn elapsed(&self) -> f64 {
        movie_time(self.id)
    }
    pub fn start(&self) {
        play_movie(self.id);
    }
}
impl Drop for BrowserMovie {
    fn drop(&mut self) {
        close_movie(self.id);
    }
}
