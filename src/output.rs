pub mod auto;
pub mod fake;
pub mod file;
pub mod rtmp;

use crate::mixer::Error as MixerError;
use crate::Result;
use crate::{AudioConfig, AudioEncoderConfig, Mux, VideoConfig, VideoEncoderConfig};

pub use auto::Auto;
pub use fake::Fake;
pub use file::File;
use gst::prelude::*;
use gstreamer as gst;
pub use rtmp::RTMP;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Config {
    pub name: String,
    pub video: VideoConfig,
    pub audio: AudioConfig,
    #[serde(default)]
    pub encoder: EncoderConfig,
    #[serde(default)]
    pub mux: Option<Mux>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct EncoderConfig {
    #[serde(default)]
    pub audio: AudioEncoderConfig,
    #[serde(default)]
    pub video: VideoEncoderConfig,
}

impl Default for EncoderConfig {
    fn default() -> Self {
        Self {
            audio: AudioEncoderConfig::default(),
            video: VideoEncoderConfig::default(),
        }
    }
}

pub enum Output {
    RTMP(RTMP),
    Auto(Auto),
    Fake(Fake),
    File(File),
}

impl Output {
    pub fn create_rtmp(config: Config, location: &str) -> Result<Self> {
        RTMP::create(config, location).map(Self::RTMP)
    }

    pub fn create_auto(config: Config) -> Result<Self> {
        Auto::create(config).map(Self::Auto)
    }

    pub fn create_fake(config: Config) -> Result<Self> {
        Fake::create(config).map(Self::Fake)
    }

    pub fn create_file(config: Config, location: &str) -> Result<Self> {
        File::create(config, location).map(Self::File)
    }

    pub fn name(&self) -> String {
        match self {
            Output::RTMP(output) => output.name(),
            Output::Auto(output) => output.name(),
            Output::Fake(output) => output.name(),
            Output::File(output) => output.name(),
        }
    }

    pub fn output_type(&self) -> String {
        match self {
            Output::RTMP(_) => "RTMP".to_string(),
            Output::Auto(_) => "Auto".to_string(),
            Output::Fake(_) => "Fake".to_string(),
            Output::File(_) => "File".to_string(),
        }
    }

    pub fn location(&self) -> String {
        match self {
            Output::RTMP(output) => output.location.clone(),
            Output::Auto(_) => "".to_string(),
            Output::Fake(_) => "".to_string(),
            Output::File(_) => "".to_string(),
        }
    }

    pub fn link(
        &mut self,
        pipeline: gst::Pipeline,
        audio: gst::Element,
        video: gst::Element,
    ) -> Result<()> {
        match self {
            Output::RTMP(output) => output.link(pipeline, audio, video),
            Output::Auto(output) => output.link(pipeline, audio, video),
            Output::Fake(output) => output.link(pipeline, audio, video),
            Output::File(output) => output.link(pipeline, audio, video),
        }
    }

    pub fn unlink(&self) -> Result<()> {
        match self {
            Output::RTMP(output) => output.unlink(),
            Output::Auto(output) => output.unlink(),
            Output::Fake(output) => output.unlink(),
            Output::File(output) => output.unlink(),
        }
    }

    pub fn set_state(&mut self, state: gst::State) -> Result<()> {
        match self {
            Output::RTMP(output) => output.set_state(state),
            Output::Auto(output) => output.set_state(state),
            Output::Fake(output) => output.set_state(state),
            Output::File(output) => output.set_state(state),
        }
    }
}

fn release_request_pad(elem: &gst::Element) -> Result<()> {
    let pad = elem.get_static_pad("sink").ok_or_else(|| {
        MixerError::Gstreamer("Failed to get static sink pad for element".to_string())
    })?;
    if pad.is_linked() {
        let peer_pad = pad.get_peer().ok_or_else(|| {
            MixerError::Gstreamer("Could not retrieve peer pad for sink element".to_string())
        })?;
        peer_pad
            .get_parent_element()
            .ok_or_else(|| {
                MixerError::Gstreamer("Failed to get parent element for peer pad".to_string())
            })?
            .release_request_pad(&peer_pad);
    }

    Ok(())
}
