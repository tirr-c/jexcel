use std::mem::MaybeUninit;
use std::ops::{Deref, DerefMut};
use std::ptr::NonNull;

mod encoder_frame;
mod error;
mod frame_settings;
mod parallel_runner;
pub mod sys;

pub use encoder_frame::*;
pub use error::{Error, Result};
pub use frame_settings::*;
pub use sys::JxlBasicInfo as BasicInfoData;

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
    close_state: CloseState,
}

impl JxlEncoder {
    pub fn new() -> Option<Self> {
        unsafe {
            let encoder = sys::JxlEncoderCreate(std::ptr::null_mut());
            sys::JxlEncoderSetParallelRunner(
                encoder,
                Some(parallel_runner::rayon_parallel_runner),
                std::ptr::null_mut(),
            );
            let encoder = NonNull::new(encoder)?;
            Some(Self {
                encoder,
                frame_settings: Vec::new(),
                close_state: CloseState::Open,
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

    pub fn set_icc_profile(&mut self, icc: &[u8]) -> Result<()> {
        unsafe {
            let _ret = sys::JxlEncoderSetICCProfile(self.encoder.as_ptr(), icc.as_ptr(), icc.len());
            Error::try_from_libjxl_encoder(self.encoder)
        }
    }

    pub fn set_jpeg_reconstruction(&mut self, store_jpeg_metadata: bool) -> Result<()> {
        let store_jpeg_metadata = if store_jpeg_metadata {
            sys::JXL_TRUE
        } else {
            sys::JXL_FALSE
        };
        unsafe {
            let _ret =
                sys::JxlEncoderStoreJPEGMetadata(self.encoder.as_ptr(), store_jpeg_metadata as i32);
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

    pub fn add_frame(&mut self, settings_key: FrameSettingsKey) -> Result<EncoderFrame> {
        EncoderFrame::new(self, settings_key)
    }

    pub fn close_frames(&mut self) {
        unsafe {
            sys::JxlEncoderCloseFrames(self.encoder.as_ptr());
            self.close_state = CloseState::FramesClosed;
        }
    }

    pub fn close_input(&mut self) {
        unsafe {
            sys::JxlEncoderCloseInput(self.encoder.as_ptr());
            self.close_state = CloseState::InputClosed;
        }
    }

    pub fn pull_outputs(&mut self, buffer: &mut [u8]) -> Result<OutputStatus> {
        let mut bytes_avail = buffer.len();
        if bytes_avail < 32 {
            return Ok(OutputStatus {
                bytes_written: 0,
                need_more_output: true,
            });
        }

        let mut buffer_ptr = buffer.as_mut_ptr();
        let mut need_more_output = true;
        unsafe {
            while bytes_avail >= 32 {
                let ret = sys::JxlEncoderProcessOutput(
                    self.encoder.as_ptr(),
                    &mut buffer_ptr,
                    &mut bytes_avail,
                );
                if ret == sys::JxlEncoderStatus_JXL_ENC_SUCCESS {
                    need_more_output = false;
                    break;
                }
                if ret == sys::JxlEncoderStatus_JXL_ENC_ERROR {
                    Error::try_from_libjxl_encoder(self.encoder)?;
                    // Fallback error code
                    return Err(Error::BadInput);
                }
            }
        }

        Ok(OutputStatus {
            bytes_written: buffer.len() - bytes_avail,
            need_more_output,
        })
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

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum CloseState {
    Open,
    FramesClosed,
    InputClosed,
}

#[derive(Debug)]
pub struct OutputStatus {
    bytes_written: usize,
    need_more_output: bool,
}

impl OutputStatus {
    pub fn bytes_written(&self) -> usize {
        self.bytes_written
    }

    pub fn need_more_output(&self) -> bool {
        self.need_more_output
    }
}

#[derive(Debug)]
pub struct JxlDecoder {
    decoder: NonNull<sys::JxlDecoder>,
}

impl JxlDecoder {
    pub fn new() -> Option<Self> {
        unsafe {
            let decoder = sys::JxlDecoderCreate(std::ptr::null_mut());
            sys::JxlDecoderSetParallelRunner(
                decoder,
                Some(parallel_runner::rayon_parallel_runner),
                std::ptr::null_mut(),
            );
            let decoder = NonNull::new(decoder)?;
            Some(Self { decoder })
        }
    }

    pub fn decode_to_pixels(
        &mut self,
        input_buf: &[u8],
        num_channels: u32,
        sample_format: SampleFormat,
    ) -> Result<Vec<u8>> {
        let dec = self.decoder.as_ptr();

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
            sys::JxlDecoderReset(dec);

            let ret = sys::JxlDecoderSubscribeEvents(
                dec,
                sys::JxlDecoderStatus_JXL_DEC_FULL_IMAGE as i32,
            );
            Error::try_from_libjxl_decoder(ret)?;

            let ret = sys::JxlDecoderSetKeepOrientation(dec, sys::JXL_TRUE as i32);
            Error::try_from_libjxl_decoder(ret)?;

            let ret = sys::JxlDecoderSetInput(dec, input_buf.as_ptr(), input_buf.len());
            Error::try_from_libjxl_decoder(ret)?;

            let ret = sys::JxlDecoderProcessInput(dec);
            if ret != sys::JxlDecoderStatus_JXL_DEC_NEED_IMAGE_OUT_BUFFER {
                return Err(Error::Unknown);
            }

            let mut buffer_len = 0usize;
            let ret = sys::JxlDecoderImageOutBufferSize(dec, &pixel_format, &mut buffer_len);
            Error::try_from_libjxl_decoder(ret)?;

            let mut out_buf = vec![0u8; buffer_len];
            let ret = sys::JxlDecoderSetImageOutBuffer(
                dec,
                &pixel_format,
                out_buf.as_mut_ptr().cast(),
                buffer_len,
            );
            Error::try_from_libjxl_decoder(ret)?;

            loop {
                let ret = sys::JxlDecoderProcessInput(dec);
                match ret {
                    sys::JxlDecoderStatus_JXL_DEC_FULL_IMAGE => break,
                    sys::JxlDecoderStatus_JXL_DEC_SUCCESS
                    | sys::JxlDecoderStatus_JXL_DEC_ERROR
                    | sys::JxlDecoderStatus_JXL_DEC_NEED_MORE_INPUT => {
                        return Err(Error::Unknown);
                    }
                    _ => {}
                }
            }

            sys::JxlDecoderReleaseInput(dec);

            Ok(out_buf)
        }
    }

    pub fn decode_to_jpeg(&mut self, input_buf: &[u8]) -> Result<Vec<u8>> {
        let dec = self.decoder.as_ptr();

        unsafe {
            sys::JxlDecoderReset(dec);

            let ret = sys::JxlDecoderSubscribeEvents(
                dec,
                (sys::JxlDecoderStatus_JXL_DEC_JPEG_RECONSTRUCTION
                    | sys::JxlDecoderStatus_JXL_DEC_FULL_IMAGE) as i32,
            );
            Error::try_from_libjxl_decoder(ret)?;

            let ret = sys::JxlDecoderSetInput(dec, input_buf.as_ptr(), input_buf.len());
            Error::try_from_libjxl_decoder(ret)?;

            let ret = sys::JxlDecoderProcessInput(dec);
            if ret != sys::JxlDecoderStatus_JXL_DEC_JPEG_RECONSTRUCTION {
                tracing::debug!(?ret);
                return Err(Error::Unknown);
            }

            let mut output = Vec::<u8>::with_capacity(1 << 20);
            let ret =
                sys::JxlDecoderSetJPEGBuffer(dec, output.as_mut_ptr().cast(), output.capacity());
            Error::try_from_libjxl_decoder(ret)?;

            loop {
                let ret = sys::JxlDecoderProcessInput(dec);
                match ret {
                    sys::JxlDecoderStatus_JXL_DEC_FULL_IMAGE => break,
                    sys::JxlDecoderStatus_JXL_DEC_SUCCESS
                    | sys::JxlDecoderStatus_JXL_DEC_ERROR
                    | sys::JxlDecoderStatus_JXL_DEC_NEED_MORE_INPUT => {
                        return Err(Error::Unknown);
                    }
                    sys::JxlDecoderStatus_JXL_DEC_JPEG_NEED_MORE_OUTPUT => {
                        let bytes_unused = sys::JxlDecoderReleaseJPEGBuffer(dec);
                        let output_ptr = output.capacity() - bytes_unused;
                        output.set_len(output_ptr);
                        output.reserve(bytes_unused + (1 << 20));

                        let uninit = output.spare_capacity_mut();
                        let ret = sys::JxlDecoderSetJPEGBuffer(
                            dec,
                            uninit.as_mut_ptr().cast(),
                            uninit.len(),
                        );
                        Error::try_from_libjxl_decoder(ret)?;
                    }
                    _ => {}
                }
            }

            let bytes_unused = sys::JxlDecoderReleaseJPEGBuffer(dec);
            let bytes_written = output.capacity() - bytes_unused;
            output.set_len(bytes_written);

            sys::JxlDecoderReleaseInput(dec);

            Ok(output)
        }
    }
}

impl Drop for JxlDecoder {
    fn drop(&mut self) {
        unsafe {
            sys::JxlDecoderDestroy(self.decoder.as_ptr());
        }
    }
}
