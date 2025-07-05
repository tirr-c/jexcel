use std::ptr::NonNull;

use crate::sys;
use crate::{Error, FrameSettingsKey, JxlEncoder, Result};

#[derive(Debug)]
pub struct EncoderFrame<'encoder> {
    encoder: &'encoder mut JxlEncoder,
    settings: Option<NonNull<sys::JxlEncoderFrameSettings>>,
}

impl<'encoder> EncoderFrame<'encoder> {
    pub(crate) fn new(encoder: &'encoder mut JxlEncoder, settings_key: FrameSettingsKey) -> Result<Self> {
        let settings = settings_key.try_index_raw(encoder)?;
        Ok(Self { encoder, settings: Some(settings) })
    }
}

impl EncoderFrame<'_> {
    pub fn color_channels(&mut self, num_channels: u32, sample_format: SampleFormat, buffer: &[u8]) -> Result<&mut Self> {
        let Some(settings) = self.settings.take() else {
            return Err(Error::ApiUsage);
        };

        let size = buffer.len();
        let buffer_ptr = buffer.as_ptr();

        let pixel_format = sys::JxlPixelFormat {
            num_channels,
            data_type: match sample_format {
                SampleFormat::U8 => sys::JxlDataType_JXL_TYPE_UINT8,
                SampleFormat::U16 => sys::JxlDataType_JXL_TYPE_UINT16,
                SampleFormat::F16 => sys::JxlDataType_JXL_TYPE_FLOAT16,
                SampleFormat::F32 => sys::JxlDataType_JXL_TYPE_FLOAT,
            },
            endianness: sys::JxlEndianness_JXL_NATIVE_ENDIAN,
            align: 0,
        };

        unsafe {
            let _ret = sys::JxlEncoderAddImageFrame(settings.as_ptr(), &pixel_format, buffer_ptr as *const _, size);
            Error::try_from_libjxl_encoder(self.encoder.encoder)?;
        }

        Ok(self)
    }

    pub fn jpeg(&mut self, buffer: &[u8]) -> Result<&mut Self> {
        let Some(settings) = self.settings.take() else {
            return Err(Error::ApiUsage);
        };

        let size = buffer.len();
        let buffer_ptr = buffer.as_ptr();

        unsafe {
            let _ret = sys::JxlEncoderAddJPEGFrame(settings.as_ptr(), buffer_ptr, size);
            Error::try_from_libjxl_encoder(self.encoder.encoder)?;
        }

        Ok(self)
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum SampleFormat {
    U8,
    U16,
    F16,
    F32,
}
