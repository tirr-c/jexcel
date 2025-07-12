use std::mem::MaybeUninit;
use std::ops::{Deref, DerefMut};
use std::ptr::NonNull;

use super::sys;
use super::{Error, JxlEncoder, Result};

pub use sys::JxlFrameHeader as FrameHeaderData;

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct FrameSettingsKey(NonNull<sys::JxlEncoder>, usize);

impl FrameSettingsKey {
    #[inline]
    pub fn is_for_encoder(self, encoder: &JxlEncoder) -> bool {
        self.0 == encoder.encoder
    }

    pub(crate) fn try_index(self, encoder: &mut JxlEncoder) -> Result<FrameSettings> {
        if !self.is_for_encoder(encoder) {
            return Err(Error::Unknown);
        }

        let settings =
            unsafe { FrameSettings::from_raw(encoder.encoder, encoder.frame_settings[self.1]) };
        Ok(settings)
    }

    pub(crate) fn try_index_raw(
        self,
        encoder: &mut JxlEncoder,
    ) -> Result<NonNull<sys::JxlEncoderFrameSettings>> {
        if !self.is_for_encoder(encoder) {
            return Err(Error::Unknown);
        }

        Ok(encoder.frame_settings[self.1])
    }
}

pub struct FrameSettings<'encoder> {
    encoder: NonNull<sys::JxlEncoder>,
    settings: NonNull<sys::JxlEncoderFrameSettings>,
    _phantom: std::marker::PhantomData<&'encoder mut ()>,
}

impl<'encoder> FrameSettings<'encoder> {
    pub fn new(
        encoder: &'encoder mut JxlEncoder,
        source: Option<FrameSettingsKey>,
    ) -> Result<(Self, FrameSettingsKey)> {
        let next_key = FrameSettingsKey(encoder.encoder, encoder.frame_settings.len());
        let source_ptr = if let Some(FrameSettingsKey(base_encoder, idx)) = source {
            if base_encoder != encoder.encoder {
                return Err(Error::Unknown);
            }
            encoder.frame_settings[idx].as_ptr()
        } else {
            std::ptr::null_mut()
        };

        let settings = unsafe {
            let ptr = sys::JxlEncoderFrameSettingsCreate(encoder.encoder.as_ptr(), source_ptr);
            NonNull::new(ptr).ok_or(Error::OutOfMemory)?
        };

        encoder.frame_settings.push(settings);
        let this = Self {
            encoder: encoder.encoder,
            settings,
            _phantom: Default::default(),
        };
        Ok((this, next_key))
    }

    #[inline]
    unsafe fn from_raw(
        encoder: NonNull<sys::JxlEncoder>,
        settings: NonNull<sys::JxlEncoderFrameSettings>,
    ) -> Self {
        Self {
            encoder,
            settings,
            _phantom: Default::default(),
        }
    }
}

impl FrameSettings<'_> {
    #[inline]
    fn set_raw_i64(&mut self, option: sys::JxlEncoderFrameSettingId, value: i64) -> Result<()> {
        unsafe {
            let _ret = sys::JxlEncoderFrameSettingsSetOption(self.settings.as_ptr(), option, value);
            Error::try_from_libjxl_encoder(self.encoder)
        }
    }

    #[inline]
    #[expect(unused)]
    fn set_raw_f32(&mut self, option: sys::JxlEncoderFrameSettingId, value: f32) -> Result<()> {
        unsafe {
            let _ret =
                sys::JxlEncoderFrameSettingsSetFloatOption(self.settings.as_ptr(), option, value);
            Error::try_from_libjxl_encoder(self.encoder)
        }
    }

    pub fn frame_header(&mut self, frame_header: &FrameHeader) -> Result<&mut Self> {
        unsafe {
            let _ret = sys::JxlEncoderSetFrameHeader(self.settings.as_ptr(), &frame_header.0);
            Error::try_from_libjxl_encoder(self.encoder)?;
        }
        Ok(self)
    }

    pub fn effort(&mut self, effort: Effort) -> &mut Self {
        self.set_raw_i64(
            sys::JxlEncoderFrameSettingId_JXL_ENC_FRAME_SETTING_EFFORT,
            effort as i64,
        )
        .unwrap();
        self
    }

    /// Setting distance smaller than 0.01 will trigger lossless encoding.
    pub fn distance(&mut self, distance: f32) -> Result<&mut Self> {
        unsafe {
            if distance < 0.01 {
                sys::JxlEncoderSetFrameLossless(self.settings.as_ptr(), sys::JXL_TRUE as i32);
            } else {
                sys::JxlEncoderSetFrameDistance(self.settings.as_ptr(), distance);
            }
            Error::try_from_libjxl_encoder(self.encoder)?;
        }

        Ok(self)
    }

    pub fn modular_progressive(&mut self, progressive: Option<bool>) -> &mut Self {
        let progressive = progressive.map(|x| x as i64).unwrap_or(-1);
        self.set_raw_i64(
            sys::JxlEncoderFrameSettingId_JXL_ENC_FRAME_SETTING_RESPONSIVE,
            progressive,
        )
        .unwrap();
        self
    }

    pub fn vardct_progressive_lf(&mut self, lf_level: Option<u32>) -> Result<&mut Self> {
        let lf_level = if let Some(lf_level) = lf_level {
            if !(0..=2).contains(&lf_level) {
                return Err(Error::ApiUsage);
            }
            lf_level as i64
        } else {
            -1i64
        };

        self.set_raw_i64(
            sys::JxlEncoderFrameSettingId_JXL_ENC_FRAME_SETTING_PROGRESSIVE_DC,
            lf_level,
        )?;

        Ok(self)
    }

    pub fn vardct_progressive_hf(&mut self, progressive: Option<bool>) -> &mut Self {
        let progressive = progressive.map(|x| x as i64).unwrap_or(-1);
        self.set_raw_i64(
            sys::JxlEncoderFrameSettingId_JXL_ENC_FRAME_SETTING_PROGRESSIVE_AC,
            progressive,
        )
        .unwrap();
        self
    }

    pub fn vardct_progressive_hf_quant(&mut self, progressive: Option<bool>) -> &mut Self {
        let progressive = progressive.map(|x| x as i64).unwrap_or(-1);
        self.set_raw_i64(
            sys::JxlEncoderFrameSettingId_JXL_ENC_FRAME_SETTING_QPROGRESSIVE_AC,
            progressive,
        )
        .unwrap();
        self
    }

    pub fn modular(&mut self, modular: Option<bool>) -> &mut Self {
        let modular = modular.map(|x| x as i64).unwrap_or(-1);
        self.set_raw_i64(
            sys::JxlEncoderFrameSettingId_JXL_ENC_FRAME_SETTING_MODULAR,
            modular,
        )
        .unwrap();
        self
    }

    pub fn decoding_speed(&mut self, speed: u32) -> Result<&mut Self> {
        self.set_raw_i64(
            sys::JxlEncoderFrameSettingId_JXL_ENC_FRAME_SETTING_DECODING_SPEED,
            speed as i64,
        )?;
        Ok(self)
    }
}

#[derive(Debug)]
pub struct FrameHeader(FrameHeaderData);

impl Default for FrameHeader {
    fn default() -> Self {
        Self::new()
    }
}

impl Deref for FrameHeader {
    type Target = FrameHeaderData;

    fn deref(&self) -> &FrameHeaderData {
        &self.0
    }
}

impl DerefMut for FrameHeader {
    fn deref_mut(&mut self) -> &mut FrameHeaderData {
        &mut self.0
    }
}

impl FrameHeader {
    pub fn new() -> Self {
        let mut frame_header = MaybeUninit::uninit();
        unsafe {
            sys::JxlEncoderInitFrameHeader(frame_header.as_mut_ptr());
            Self(frame_header.assume_init())
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[repr(i64)]
pub enum Effort {
    Lightning = 1,
    Thunder = 2,
    Falcon = 3,
    Cheetah = 4,
    Hare = 5,
    Wombat = 6,
    #[default]
    Squirrel = 7,
    Kitten = 8,
    Tortoise = 9,
    Glacier = 10,
    TectonicPlate = 11,
}

impl TryFrom<i64> for Effort {
    type Error = Error;

    fn try_from(value: i64) -> Result<Self> {
        if (1..=11).contains(&value) {
            // SAFETY: Effort has repr of i64, with valid range of 1..=11.
            let value = unsafe { std::mem::transmute::<i64, Self>(value) };
            Ok(value)
        } else {
            Err(Error::ApiUsage)
        }
    }
}
