use std::collections::HashMap;

use camino::Utf8PathBuf;
use once_cell::sync::OnceCell;
use polars::prelude::LazyFrame;

use crate::common::{DepthFrame, ImageFrame};
use crate::core::stage::{Context, Stage};
use crate::ingest::rosbag1_ingestor::{Rosbag1Ingestor, Rosbag1IngestorConfig};

#[derive(Clone)]
pub struct IngestFixture {
    dataset: HashMap<String, LazyFrame>,
    image_data: HashMap<String, Vec<ImageFrame>>,
    depth_data: HashMap<String, Vec<DepthFrame>>,
    rosbag_path: Utf8PathBuf,
}

impl IngestFixture {
    pub fn dataset(&self) -> &HashMap<String, LazyFrame> {
        &self.dataset
    }

    pub fn image_data(&self) -> &HashMap<String, Vec<ImageFrame>> {
        &self.image_data
    }

    pub fn depth_data(&self) -> &HashMap<String, Vec<DepthFrame>> {
        &self.depth_data
    }

    pub fn rosbag_path(&self) -> &Utf8PathBuf {
        &self.rosbag_path
    }
}

static INGEST_FIXTURE: OnceCell<IngestFixture> = OnceCell::new();

pub fn ingest_fixture() -> &'static IngestFixture {
    INGEST_FIXTURE.get_or_init(|| {
        let manifest_dir = Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let rosbag_path = manifest_dir.join("../airoa-moma-raw/000730/data.bag");

        let mut context = Context::default();
        context.set_rosbag_path(rosbag_path.clone());

        let mut ingestor = Rosbag1Ingestor::new(Rosbag1IngestorConfig::new());
        let mut context = ingestor
            .run(context)
            .expect("failed to run rosbag ingestor for tests");

        let dataset = context
            .dataset
            .take()
            .expect("ingestor must populate dataset");
        let image_data = context.image_data.take().unwrap_or_default();
        let depth_data = context.depth_data.take().unwrap_or_default();

        IngestFixture {
            dataset,
            image_data,
            depth_data,
            rosbag_path,
        }
    })
}

pub fn ingest_dataset_clone() -> HashMap<String, LazyFrame> {
    ingest_fixture().dataset.clone()
}

pub fn image_data_clone() -> HashMap<String, Vec<ImageFrame>> {
    ingest_fixture().image_data.clone()
}

pub fn depth_data_clone() -> HashMap<String, Vec<DepthFrame>> {
    ingest_fixture().depth_data.clone()
}

pub fn rosbag_path() -> &'static Utf8PathBuf {
    ingest_fixture().rosbag_path()
}
