use std::ffi::c_void;
use std::ptr::NonNull;

use crate::sys;

#[derive(Copy, Clone)]
struct UnsafeAssumeSendSync<T>(T);
unsafe impl<T> Send for UnsafeAssumeSendSync<T> {}
unsafe impl<T> Sync for UnsafeAssumeSendSync<T> {}

impl<T> UnsafeAssumeSendSync<T> {
    unsafe fn new(value: T) -> Self {
        Self(value)
    }

    fn into_inner(self) -> T {
        self.0
    }
}

pub(crate) unsafe extern "C" fn rayon_parallel_runner(
    pool: *mut c_void,
    jxl_opaque: *mut c_void,
    init: sys::JxlParallelRunInit,
    func: sys::JxlParallelRunFunction,
    start_range: u32,
    end_range: u32,
) -> sys::JxlParallelRetCode {
    let pool = NonNull::new(pool as *mut rayon::ThreadPool);
    let jxl_opaque = unsafe { UnsafeAssumeSendSync::new(jxl_opaque) };
    let range = start_range..end_range;

    unsafe {
        if let Some(pool) = pool {
            let pool = pool.as_ref();
            pool.install(|| run_inner(jxl_opaque, init, func, range))
        } else {
            run_inner(jxl_opaque, init, func, range)
        }
    }
}

unsafe fn run_inner(
    jxl_opaque: UnsafeAssumeSendSync<*mut c_void>,
    init: sys::JxlParallelRunInit,
    func: sys::JxlParallelRunFunction,
    range: std::ops::Range<u32>,
) -> sys::JxlParallelRetCode {
    use rayon::prelude::*;

    let Some(init) = init else {
        return sys::JXL_PARALLEL_RET_RUNNER_ERROR as sys::JxlParallelRetCode;
    };
    let Some(func) = func else {
        return sys::JXL_PARALLEL_RET_RUNNER_ERROR as sys::JxlParallelRetCode;
    };
    let func = unsafe { UnsafeAssumeSendSync::new(func) };

    let ret = unsafe { init(jxl_opaque.0, rayon::current_num_threads()) };
    if ret != 0 {
        return ret;
    }

    range.into_par_iter().for_each(|idx| unsafe {
        let func = func.into_inner();
        func(
            jxl_opaque.into_inner(),
            idx,
            rayon::current_thread_index().unwrap_or(0),
        );
    });

    sys::JXL_PARALLEL_RET_SUCCESS as sys::JxlParallelRetCode
}
