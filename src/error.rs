use std::ptr::NonNull;

use crate::sys;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("out of memory")]
    OutOfMemory,
    #[error("cannot produce JPEG bitstream reconstruction data")]
    JpegBitstreamReconstruction,
    #[error("wrong API usage")]
    ApiUsage,
    #[error("bad input")]
    BadInput,
    #[error("not supported")]
    NotSupported,
    #[error("unknown error")]
    Unknown,
}

impl Error {
    pub(crate) unsafe fn try_from_libjxl_encoder(
        encoder: NonNull<sys::JxlEncoder>,
    ) -> Result<(), Self> {
        unsafe {
            let error = sys::JxlEncoderGetError(encoder.as_ptr());
            Err(match error {
                sys::JxlEncoderError_JXL_ENC_ERR_OK => return Ok(()),
                sys::JxlEncoderError_JXL_ENC_ERR_OOM => Self::OutOfMemory,
                sys::JxlEncoderError_JXL_ENC_ERR_JBRD => Self::JpegBitstreamReconstruction,
                sys::JxlEncoderError_JXL_ENC_ERR_API_USAGE => Self::ApiUsage,
                sys::JxlEncoderError_JXL_ENC_ERR_BAD_INPUT => Self::BadInput,
                sys::JxlEncoderError_JXL_ENC_ERR_NOT_SUPPORTED => Self::NotSupported,
                _ => Self::Unknown,
            })
        }
    }
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
