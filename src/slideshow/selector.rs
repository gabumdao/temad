use anyhow::{bail, Result};
use rand::seq::SliceRandom;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct ImageSelector {
    images: Vec<PathBuf>,
    randomize: bool,
    last: Option<PathBuf>,
}

impl ImageSelector {
    pub fn new(images: Vec<PathBuf>, randomize: bool) -> Result<Self> {
        Self::with_last(images, randomize, None)
    }

    pub fn with_last(images: Vec<PathBuf>, randomize: bool, last: Option<PathBuf>) -> Result<Self> {
        if images.is_empty() {
            bail!("No wallpaper images found in the configured directory");
        }

        let last = last.filter(|candidate| images.iter().any(|path| path == candidate));

        Ok(Self {
            images,
            randomize,
            last,
        })
    }

    pub fn next(&mut self) -> &Path {
        let chosen = if self.randomize {
            self.next_random()
        } else {
            self.next_sequential()
        };
        self.last = Some(chosen.clone());
        self.last.as_deref().unwrap()
    }

    pub fn prev(&mut self) -> &Path {
        let chosen = if self.randomize {
            self.next_random()
        } else {
            self.prev_sequential()
        };
        self.last = Some(chosen.clone());
        self.last.as_deref().unwrap()
    }

    /// Synchronize selection with the last wallpaper applied by another process.
    pub fn sync_last(&mut self, last: Option<PathBuf>) {
        self.last = last.filter(|candidate| self.images.iter().any(|path| path == candidate));
    }

    pub fn update_images(&mut self, images: Vec<PathBuf>) -> Result<()> {
        if images.is_empty() {
            bail!("No wallpaper images found in the configured directory");
        }

        let last_still_exists = self
            .last
            .as_ref()
            .is_some_and(|last| images.iter().any(|path| path == last));

        self.images = images;

        if !last_still_exists {
            self.last = None;
        }

        Ok(())
    }

    fn next_sequential(&self) -> PathBuf {
        match &self.last {
            None => self.images[0].clone(),
            Some(prev) => {
                let idx = self
                    .images
                    .iter()
                    .position(|p| p == prev)
                    .map(|i| (i + 1) % self.images.len())
                    .unwrap_or(0);
                self.images[idx].clone()
            }
        }
    }

    fn prev_sequential(&self) -> PathBuf {
        match &self.last {
            None => self.images[self.images.len() - 1].clone(),
            Some(prev) => {
                let idx = self
                    .images
                    .iter()
                    .position(|p| p == prev)
                    .map(|i| if i == 0 { self.images.len() - 1 } else { i - 1 })
                    .unwrap_or(self.images.len() - 1);
                self.images[idx].clone()
            }
        }
    }

    fn next_random(&self) -> PathBuf {
        let mut rng = rand::thread_rng();

        if self.images.len() == 1 {
            return self.images[0].clone();
        }

        loop {
            let pick = self.images.choose(&mut rng).unwrap().clone();
            if self.last.as_ref() != Some(&pick) {
                return pick;
            }
        }
    }
}
