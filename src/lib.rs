use std::mem::MaybeUninit;
use std::ops::{Deref, DerefMut};
use std::ptr::NonNull;

mod error;
mod frame_settings;
pub mod sys;
mod parallel_runner;

pub use sys::JxlBasicInfo as BasicInfoData;
pub use error::{Error, Result};
pub use frame_settings::*;

#[derive(Debug)]
pub struct BasicInfo(BasicInfoData);

impl Default for BasicInfo {
    fn default() -> Self {
        Self::new()
    }
}

impl Deref for BasicInfo {
    type Target = BasicInfoData;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for BasicInfo {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl BasicInfo {
    pub fn new() -> Self {
        let mut basic_info = MaybeUninit::uninit();
        unsafe {
            sys::JxlEncoderInitBasicInfo(basic_info.as_mut_ptr());
            Self(basic_info.assume_init())
        }
    }
}

#[derive(Debug)]
pub struct ColorEncoding(sys::JxlColorEncoding);

impl ColorEncoding {
    pub fn srgb(intent: RenderingIntent) -> Self {
        Self(sys::JxlColorEncoding {
            color_space: sys::JxlColorSpace_JXL_COLOR_SPACE_RGB,
            white_point: sys::JxlWhitePoint_JXL_WHITE_POINT_D65,
            white_point_xy: Default::default(),
            primaries: sys::JxlPrimaries_JXL_PRIMARIES_SRGB,
            primaries_red_xy: Default::default(),
            primaries_green_xy: Default::default(),
            primaries_blue_xy: Default::default(),
            transfer_function: sys::JxlTransferFunction_JXL_TRANSFER_FUNCTION_SRGB,
            gamma: Default::default(),
            rendering_intent: intent.into(),
        })
    }

    pub fn srgb_linear(intent: RenderingIntent) -> Self {
        Self(sys::JxlColorEncoding {
            color_space: sys::JxlColorSpace_JXL_COLOR_SPACE_RGB,
            white_point: sys::JxlWhitePoint_JXL_WHITE_POINT_D65,
            white_point_xy: Default::default(),
            primaries: sys::JxlPrimaries_JXL_PRIMARIES_SRGB,
            primaries_red_xy: Default::default(),
            primaries_green_xy: Default::default(),
            primaries_blue_xy: Default::default(),
            transfer_function: sys::JxlTransferFunction_JXL_TRANSFER_FUNCTION_LINEAR,
            gamma: Default::default(),
            rendering_intent: intent.into(),
        })
    }
}

#[derive(Debug)]
pub enum RenderingIntent {
    Perceptual,
    Relative,
    Saturation,
    Absolute,
}

impl From<RenderingIntent> for sys::JxlRenderingIntent {
    fn from(value: RenderingIntent) -> Self {
        match value {
            RenderingIntent::Perceptual => sys::JxlRenderingIntent_JXL_RENDERING_INTENT_PERCEPTUAL,
            RenderingIntent::Relative => sys::JxlRenderingIntent_JXL_RENDERING_INTENT_RELATIVE,
            RenderingIntent::Saturation => sys::JxlRenderingIntent_JXL_RENDERING_INTENT_SATURATION,
            RenderingIntent::Absolute => sys::JxlRenderingIntent_JXL_RENDERING_INTENT_ABSOLUTE,
        }
    }
}

#[derive(Debug)]
pub struct JxlEncoder {
    encoder: NonNull<sys::JxlEncoder>,
    frame_settings: Vec<NonNull<sys::JxlEncoderFrameSettings>>,
}

impl JxlEncoder {
    pub fn new() -> Option<Self> {
        unsafe {
            let encoder = sys::JxlEncoderCreate(std::ptr::null_mut());
            sys::JxlEncoderSetParallelRunner(encoder, Some(parallel_runner::rayon_parallel_runner), std::ptr::null_mut());
            let encoder = NonNull::new(encoder)?;
            Some(Self {
                encoder,
                frame_settings: Vec::new(),
            })
        }
    }

    pub fn set_basic_info(&mut self, basic_info: &BasicInfo) -> Result<()> {
        unsafe {
            let _ret = sys::JxlEncoderSetBasicInfo(self.encoder.as_ptr(), &basic_info.0);
            Error::try_from_libjxl_encoder(self.encoder)
        }
    }

    pub fn set_color_encoding(&mut self, color_encoding: &ColorEncoding) -> Result<()> {
        unsafe {
            let _ret = sys::JxlEncoderSetColorEncoding(self.encoder.as_ptr(), &color_encoding.0);
            Error::try_from_libjxl_encoder(self.encoder)
        }
    }

    pub fn create_frame_settings_with<'encoder>(
        &'encoder mut self,
        f: impl FnOnce(&mut FrameSettings<'encoder>) -> Result<()>,
    ) -> Result<FrameSettingsKey> {
        let (mut settings, key) = FrameSettings::new(self, None)?;
        f(&mut settings)?;
        Ok(key)
    }

    pub fn clone_modify_frame_settings_with<'encoder>(
        &'encoder mut self,
        source: FrameSettingsKey,
        f: impl FnOnce(&mut FrameSettings<'encoder>) -> Result<()>,
    ) -> Result<FrameSettingsKey> {
        let (mut settings, key) = FrameSettings::new(self, Some(source))?;
        f(&mut settings)?;
        Ok(key)
    }

    pub fn update_frame_settings_with<'encoder>(
        &'encoder mut self,
        settings_key: FrameSettingsKey,
        f: impl FnOnce(&mut FrameSettings<'encoder>) -> Result<()>,
    ) -> Result<()> {
        let mut settings = settings_key.try_index(self)?;
        f(&mut settings)?;
        Ok(())
    }
}

impl Drop for JxlEncoder {
    fn drop(&mut self) {
        unsafe {
            // Will drop all frame settings.
            sys::JxlEncoderDestroy(self.encoder.as_ptr());
        }
    }
}
