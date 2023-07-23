use std::collections::HashMap;
use std::ops::Not;
use std::path::PathBuf;

use bincode::{deserialize, serialize};
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

type Result<T> = std::result::Result<T, crate::Error>;

pub(crate) struct MetaData {
    // HashMap<start, (end, completed)>
    pub(crate) sections: HashMap<usize, (usize, bool)>,
    file_path: PathBuf,
}

impl MetaData {
    pub(crate) async fn new(sections: &Vec<(usize, usize)>, file_path: &PathBuf) -> Result<Self> {
        if file_path.exists() {
            // if loading existing metadata fails, create new metadata
            if let Ok(s) = Self::load(file_path).await {
                return Ok(s);
            }
        }

        let mut map = HashMap::new();

        // create sections map
        for (start, end) in sections {
            map.insert(*start, (*end, false));
        }

        // no metadata file is created on the disk until the first section is completed
        Ok(Self {
            sections: map,
            file_path: file_path.clone(),
        })
    }

    // load metadata from file
    pub(crate) async fn load(file_path: &PathBuf) -> Result<Self> {
        let mut file = File::open(&file_path).await?; // open file
        let mut buffer = Vec::new(); // create buffer for data
        file.read_to_end(&mut buffer).await?; // read file in buffer

        // deserialize buffer into HashMap
        let sections = deserialize(&buffer)?;

        Ok(Self {
            sections,
            file_path: file_path.clone(),
        })
    }

    // save metadata to file
    pub(crate) async fn save(&self) -> Result<()> {
        let mut file = File::create(&self.file_path).await?; // create file
        let buffer = serialize(&self.sections).unwrap(); // serialize HashMap into buffer
        file.write_all(&buffer).await?; // write buffer to file

        Ok(())
    }

    // complete a section
    pub(crate) async fn complete(&mut self, start: usize) -> Result<()> {
        self.sections.insert(start, (0, true)); // set section as complete, end is not needed anymore
        self.save().await // save metadata to file
    }

    // get a list of incomplete section starts
    pub(crate) fn incomplete_sections(&self) -> Vec<usize> {
        self.sections.iter()
            .filter(|(_, (_, complete))| complete.not()) // filter out complete sections
            .map(|(start, _)| *start) // convert to owned usize
            .collect()
    }
}
