use core::ptr::slice_from_raw_parts;

/// Physical memory represented in bytes which is 64MB
pub const SDRAM_SIZE: usize = 0x4000000;
const SDRAM_BASE_ADDRESS: usize = 0xC0000000;

/// Returns a reference to a slice of `len` elements with a given `offset` in type `T` if it fits into the SDRAM
/// of the Daisy Seed Rev. 5 (which is 64MB).
///
/// ## Safety
/// The caller needs to know in which format the information is stored in the SDRAM. Internally, an unsafe function
/// is used which potentially returns back corrupted data.
///
/// This function is thread safe since it only returns a reading reference to a certian area in memory. It is the caller's job
/// to make sure that valid information is being stored there.
pub fn get_slice<T>(offset: usize, len: usize) -> Option<&'static [T]> {
    if sized::<T>(offset + len) < SDRAM_SIZE {
        unsafe {
            Some(
                slice_from_raw_parts((SDRAM_BASE_ADDRESS + sized::<T>(offset)) as *mut T, len)
                    .as_ref()
                    .unwrap_unchecked(),
            )
        }
    } else {
        None
    }
}

fn sized<T>(value: usize) -> usize {
    value * core::mem::size_of::<T>()
}
