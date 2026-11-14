//! Values a mod keeps between frames. wasvy starts a fresh instance of the mod for every system
//! call, so statics and `thread_local!`s are gone by the next frame; the host's memory keeps
//! lists of numbers instead, and `Persist` turns a mod's own type into one.

/// A type the host can keep for the session as a list of numbers.
pub trait Persist: Sized {
    fn pack(&self) -> Vec<f32>;
    /// The value back from what `pack` made; `None` for a list that does not fit (stale or
    /// corrupted), which counts as nothing kept.
    fn unpack(values: &[f32]) -> Option<Self>;
}

impl Persist for f32 {
    fn pack(&self) -> Vec<f32> {
        vec![*self]
    }

    fn unpack(values: &[f32]) -> Option<Self> {
        match values {
            &[value] => Some(value),
            _ => None,
        }
    }
}

impl<const N: usize> Persist for [f32; N] {
    fn pack(&self) -> Vec<f32> {
        self.to_vec()
    }

    fn unpack(values: &[f32]) -> Option<Self> {
        values.try_into().ok()
    }
}

impl<const N: usize> Persist for [u64; N] {
    fn pack(&self) -> Vec<f32> {
        self.iter()
            .flat_map(|value| (0..4).map(move |i| ((value >> (i * 16)) & 65535) as f32))
            .collect()
    }
    fn unpack(values: &[f32]) -> Option<Self> {
        (values.len() == N * 4
            && values
                .iter()
                .all(|v| v.is_finite() && (0.0..=65535.0).contains(v) && v.fract() == 0.0))
        .then(|| {
            std::array::from_fn(|index| {
                values[index * 4..index * 4 + 4]
                    .iter()
                    .enumerate()
                    .fold(0, |value, (i, n)| value | ((*n as u64) << (i * 16)))
            })
        })
    }
}
