use super::Config;
use crate::{
    gst_create_element, mixer,
    output::{Config as OutputConfig, EncoderConfig, File as FileOutput},
    AudioEncoder, AudioEncoderConfig, Mux, Result, VideoEncoder, VideoEncoderConfig,
};

use gst::prelude::*;
use gstreamer as gst;

pub struct URI {
    pub name: String,
    pub location: String,
    config: Config,
    pipeline: Option<gst::Pipeline>,
    source: gst::Element,
    audio_tee: gst::Element,
    audio_tee_queue: gst::Element,
    audio_convert: gst::Element,
    audio_resample: gst::Element,
    audio_volume: gst::Element,
    audio_queue: gst::Element,
    video_tee: gst::Element,
    video_tee_queue: gst::Element,
    video_convert: gst::Element,
    video_scale: gst::Element,
    video_rate: gst::Element,
    video_capsfilter: gst::Element,
    video_queue: gst::Element,
    record_output: Option<FileOutput>,
}

impl URI {
    pub fn create(config: Config, uri: &str) -> Result<Self> {
        let source = gst_create_element(
            "uridecodebin",
            &format!("input_{}_uridecodebin", config.name),
        )?;
        source.set_property("uri", &uri)?;

        let video_tee_queue =
            gst_create_element("queue2", &format!("input_{}_video_tee_queue", config.name))?;
        let video_tee = gst_create_element("tee", &format!("input_{}_video_tee", config.name))?;
        video_tee.set_property("allow-not-linked", &true)?;

        let video_convert = gst_create_element(
            "videoconvert",
            &format!("input_{}_video_convert", config.name),
        )?;
        let video_scale =
            gst_create_element("videoscale", &format!("input_{}_video_scale", config.name))?;
        let video_rate =
            gst_create_element("videorate", &format!("input_{}_video_rate", config.name))?;
        let video_caps = gst::Caps::builder("video/x-raw")
            .field("framerate", &gst::Fraction::new(config.video.framerate, 1))
            .field("format", &config.video.format.to_string())
            .field("width", &config.video.width)
            .field("height", &config.video.height)
            .field("colorimetry", &"sRGB")
            .build();
        let video_capsfilter = gst_create_element(
            "capsfilter",
            &format!("input_{}_video_capsfilter", config.name),
        )?;
        video_capsfilter.set_property("caps", &video_caps)?;

        let video_queue =
            gst_create_element("queue2", &format!("input_{}_video_queue", config.name))?;

        let audio_tee_queue =
            gst_create_element("queue2", &format!("input_{}_audio_tee_queue", config.name))?;
        let audio_tee = gst_create_element("tee", &format!("input_{}_audio_tee", config.name))?;
        audio_tee.set_property("allow-not-linked", &true)?;

        let audio_queue =
            gst_create_element("queue", &format!("input_{}_audio_queue", config.name))?;
        let audio_convert = gst_create_element(
            "audioconvert",
            &format!("input_{}_audio_convert", config.name),
        )?;
        let audio_resample = gst_create_element(
            "audioresample",
            &format!("input_{}_audio_resample", config.name),
        )?;

        let audio_volume =
            gst_create_element("volume", &format!("input_{}_audio_volume", config.name))?;
        audio_volume.set_property("volume", &config.audio.volume)?;

        let audio = audio_convert.clone();
        let video = video_convert.clone();
        let vqueue = video_queue.clone();
        let video_config = config.video.clone();
        let input_name = config.name.clone();
        source.connect_pad_added(move |src, src_pad| {
            println!(
                "Received new pad {} from {}",
                src_pad.get_name(),
                src.get_name()
            );

            let new_pad_caps = src_pad
                .get_current_caps()
                .expect("Failed to get caps of new pad.");
            let new_pad_struct = new_pad_caps
                .get_structure(0)
                .expect("Failed to get first structure of caps.");
            let new_pad_type = new_pad_struct.get_name();

            let running_time = video.get_current_running_time();

            if new_pad_type.starts_with("audio/x-raw") {
                let sink_pad = audio
                    .get_static_pad("sink")
                    .expect("Failed to get sink pad from audio mixer");
                if sink_pad.is_linked() {
                    println!("We are already linked. Ignoring.");
                    return;
                }

                // Offset src_pad by current running time. So that videos do not fast-forward to
                // get in sync with running time of pipeline.
                src_pad
                    .set_offset(gst::format::GenericFormattedValue::Time(running_time).get_value());

                let res = src_pad.link(&sink_pad);
                if res.is_err() {
                    println!("Type is {} but link failed.", new_pad_type);
                } else {
                    println!("Link succeeded (type {}).", new_pad_type);
                }
            } else if new_pad_type.starts_with("video/x-raw") {
                let sink_pad = video
                    .get_static_pad("sink")
                    .expect("Failed to get static sink pad from video_mixer");
                if sink_pad.is_linked() {
                    println!("We are already linked. Ignoring.");
                    return;
                }

                // Offset src_pad by current running time. So that videos do not fast-forward to
                // get in sync with running time of pipeline.
                src_pad
                    .set_offset(gst::format::GenericFormattedValue::Time(running_time).get_value());

                let queue_pad = match vqueue.get_static_pad("src") {
                    Some(pad) => pad,
                    None => {
                        tracing::warn!(
                            input = input_name.as_str(),
                            "Failed to retrieve static src pad for video"
                        );
                        return;
                    }
                };
                if queue_pad.is_linked() {
                    let compositor_pad = match queue_pad.get_peer() {
                        Some(pad) => pad,
                        None => {
                            tracing::warn!(
                                input = input_name.as_str(),
                                "Failed to retrieve compositor pad for video"
                            );
                            return;
                        }
                    };

                    if let Some(zorder) = video_config.zorder {
                        let _ = compositor_pad.set_property("zorder", &zorder);
                    }
                    let _ = compositor_pad.set_property("alpha", &video_config.alpha);
                    let _ = compositor_pad.set_property("xpos", &video_config.xpos);
                    let _ = compositor_pad.set_property("ypos", &video_config.ypos);
                    let _ = compositor_pad.set_property("repeat-after-eos", &video_config.repeat);
                }

                let res = src_pad.link(&sink_pad);
                if res.is_err() {
                    println!("Type is {} but link failed.", new_pad_type);
                } else {
                    println!("Link succeeded (type {}).", new_pad_type);
                }
            }
        });

        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| mixer::Error::System(e.to_string()))?
            .as_millis();

        let recording_config = OutputConfig {
            name: format!("record_{}", config.name),
            audio: config.audio.clone(),
            video: config.video.clone(),
            encoder: EncoderConfig {
                audio: AudioEncoderConfig {
                    encoder: AudioEncoder::Vorbis,
                },
                video: VideoEncoderConfig {
                    encoder: VideoEncoder::VP9,
                    profile: None,
                    preset: None,
                    speed: None,
                },
            },
            mux: Some(Mux::MKV),
        };

        let record_output = match config.record {
            true => Some(FileOutput::create(
                recording_config,
                &format!("./recordings/input_{}_{}.mkv", config.name, timestamp),
            )?),

            false => None,
        };

        Ok(Self {
            name: config.name.to_string(),
            location: config.name.to_string(),
            config,
            pipeline: None,
            source,
            audio_tee,
            audio_tee_queue,
            audio_convert,
            audio_volume,
            audio_resample,
            audio_queue,
            video_tee,
            video_tee_queue,
            video_convert,
            video_scale,
            video_rate,
            video_capsfilter,
            video_queue,
            record_output,
        })
    }

    pub fn name(&self) -> String {
        self.name.clone()
    }

    pub fn link(
        &mut self,
        pipeline: gst::Pipeline,
        audio: gst::Element,
        video: gst::Element,
    ) -> Result<()> {
        pipeline.add_many(&[
            &self.audio_tee_queue,
            &self.audio_tee,
            &self.video_tee_queue,
            &self.video_tee,
        ])?;

        if let Some(record_output) = self.record_output.as_mut() {
            record_output.link(
                pipeline.clone(),
                self.audio_tee.clone(),
                self.video_tee.clone(),
            )?;
        }

        pipeline.add_many(&[
            &self.source,
            &self.audio_convert,
            &self.audio_volume,
            &self.audio_resample,
            &self.audio_queue,
            &self.video_convert,
            &self.video_scale,
            &self.video_rate,
            &self.video_capsfilter,
            &self.video_queue,
        ])?;

        self.pipeline = Some(pipeline);

        gst::Element::link_many(&[
            &self.audio_convert,
            &self.audio_volume,
            &self.audio_resample,
            &self.audio_tee_queue,
            &self.audio_tee,
            &self.audio_queue,
            &audio,
        ])?;
        gst::Element::link_many(&[
            &self.video_convert,
            &self.video_scale,
            &self.video_rate,
            &self.video_capsfilter,
            &self.video_tee_queue,
            &self.video_tee,
            &self.video_queue,
            &video,
        ])?;

        // If we add an input and do not supply its zorder, the compositor will increment its
        // highest zorder and use that for this pad. We need to determine what that zorder is
        // and store it to the config, so that if the mixer needs to temporarily change the zorder
        // it is able to restore it back to its original state.
        let prop = self
            .video_queue
            .get_static_pad("src")
            .ok_or(mixer::Error::Gstreamer(
                "Failed to retrieve src pad for video_queue".to_string(),
            ))?
            .get_peer()
            .ok_or(mixer::Error::Gstreamer(
                "Failed to retrieve peer pad for video_queue".to_string(),
            ))?
            .get_property("zorder")?;
        let zorder = prop.downcast::<u32>().map_err(|_| mixer::Error::Unknown)?;
        self.config.video.zorder = Some(zorder.get_some());

        Ok(())
    }

    pub fn unlink(&self) -> Result<()> {
        super::release_request_pad(&self.audio_queue)?;
        super::release_request_pad(&self.video_queue)?;

        if let Some(pipeline) = self.pipeline.as_ref() {
            pipeline.remove_many(&[
                &self.source,
                &self.audio_tee,
                &self.audio_tee_queue,
                &self.audio_convert,
                &self.audio_volume,
                &self.audio_resample,
                &self.audio_queue,
                &self.video_tee,
                &self.video_tee_queue,
                &self.video_convert,
                &self.video_scale,
                &self.video_rate,
                &self.video_capsfilter,
                &self.video_queue,
            ])?;
        }

        Ok(())
    }

    pub fn set_state(&mut self, state: gst::State) -> Result<()> {
        self.source.set_state(state)?;
        self.audio_convert.set_state(state)?;
        self.audio_resample.set_state(state)?;
        self.audio_volume.set_state(state)?;
        self.audio_queue.set_state(state)?;
        self.video_convert.set_state(state)?;
        self.video_scale.set_state(state)?;
        self.video_rate.set_state(state)?;
        self.video_capsfilter.set_state(state)?;
        self.video_queue.set_state(state)?;
        Ok(())
    }

    pub fn set_volume(&mut self, volume: f64, update_config: bool) -> Result<()> {
        if update_config {
            self.config.audio.volume = volume;
        }
        self.audio_volume.set_property("volume", &volume)?;
        Ok(())
    }

    pub fn set_zorder(&mut self, zorder: u32, update_config: bool) -> Result<()> {
        if update_config {
            self.config.video.zorder = Some(zorder);
        }
        super::set_peer_pad_property(
            &self
                .video_queue
                .get_static_pad("src")
                .ok_or(mixer::Error::Gstreamer(
                    "failed to retrieve src pad".to_string(),
                ))?,
            "zorder",
            &zorder,
        )?;

        Ok(())
    }

    pub fn set_width(&mut self, width: i32, update_config: bool) -> Result<()> {
        if update_config {
            self.config.video.width = width;
        }
        super::set_peer_pad_property(
            &self
                .video_queue
                .get_static_pad("src")
                .ok_or(mixer::Error::Gstreamer(
                    "failed to retrieve src pad".to_string(),
                ))?,
            "width",
            &width,
        )?;

        Ok(())
    }

    pub fn set_height(&mut self, height: i32, update_config: bool) -> Result<()> {
        if update_config {
            self.config.video.height = height;
        }
        super::set_peer_pad_property(
            &self
                .video_queue
                .get_static_pad("src")
                .ok_or(mixer::Error::Gstreamer(
                    "failed to retrieve src pad".to_string(),
                ))?,
            "height",
            &height,
        )?;

        Ok(())
    }

    pub fn set_xpos(&mut self, xpos: i32, update_config: bool) -> Result<()> {
        if update_config {
            self.config.video.xpos = xpos;
        }
        super::set_peer_pad_property(
            &self
                .video_queue
                .get_static_pad("src")
                .ok_or(mixer::Error::Gstreamer(
                    "failed to retrieve src pad".to_string(),
                ))?,
            "xpos",
            &xpos,
        )?;

        Ok(())
    }

    pub fn set_ypos(&mut self, ypos: i32, update_config: bool) -> Result<()> {
        if update_config {
            self.config.video.ypos = ypos;
        }
        super::set_peer_pad_property(
            &self
                .video_queue
                .get_static_pad("src")
                .ok_or(mixer::Error::Gstreamer(
                    "failed to retrieve src pad".to_string(),
                ))?,
            "ypos",
            &ypos,
        )?;

        Ok(())
    }

    pub fn set_alpha(&mut self, alpha: f64, update_config: bool) -> Result<()> {
        if update_config {
            self.config.video.alpha = alpha;
        }
        super::set_peer_pad_property(
            &self
                .video_queue
                .get_static_pad("src")
                .ok_or(mixer::Error::Gstreamer(
                    "failed to retrieve src pad".to_string(),
                ))?,
            "alpha",
            &alpha,
        )?;

        Ok(())
    }

    pub fn config(&self) -> Config {
        self.config.clone()
    }
}
