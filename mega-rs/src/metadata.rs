use std::collections::HashMap;
use std::ops::Not;
use std::path::PathBuf;

use bincode::{deserialize, serialize};
use tokio::fs::File;
use tokio::io;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub struct MetaData {
    // HashMap<start, (end, completed)>
    pub sections: HashMap<usize, (usize, bool)>,
    file_path: PathBuf,
}

impl MetaData {
    pub async fn new(sections: &Vec<(usize, usize)>, file_path: &PathBuf) -> io::Result<Self> {
        if file_path.exists() {
            Self::load(file_path).await // load existing metadata
        } else {
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
    }

    // load metadata from file
    pub async fn load(file_path: &PathBuf) -> io::Result<Self> {
        let mut file = File::open(&file_path).await?; // open file
        let mut buffer = Vec::new(); // create buffer for data
        file.read_to_end(&mut buffer).await?; // read file in buffer

        // deserialize buffer into HashMap
        let sections = deserialize(&buffer).unwrap();

        Ok(Self {
            sections,
            file_path: file_path.clone(),
        })
    }

    // save metadata to file
    pub async fn save(&self) -> io::Result<()> {
        let mut file = File::create(&self.file_path).await?; // create file
        let buffer = serialize(&self.sections).unwrap(); // serialize HashMap into buffer
        file.write_all(&buffer).await?; // write buffer to file

        Ok(())
    }

    // complete a section
    pub async fn complete(&mut self, start: usize) -> io::Result<()> {
        self.sections.insert(start, (0, true)); // set section as complete, end is not needed anymore
        self.save().await // save metadata to file
    }

    // get a list of incomplete section starts
    pub fn incomplete_sections(&self) -> Vec<usize> {
        self.sections.iter()
            .filter(|(_, (_, complete))| complete.not()) // filter out complete sections
            .map(|(start, _)| *start) // convert to owned usize
            .collect()
    }
}
