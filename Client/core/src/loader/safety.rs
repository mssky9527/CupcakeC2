// Memory Safety Utilities for BOF Loader
//
// 提供安全的内存操作函数，防止缓冲区溢出和非法内存访问

use super::error::{BofError, BofResult};

/// 安全地从缓冲区读取结构体
///
/// # Safety
///
/// 验证缓冲区大小足够容纳结构体
pub unsafe fn read_struct<T>(buffer: &[u8], offset: usize) -> BofResult<&T> {
    let struct_size = std::mem::size_of::<T>();

    // 检查是否会溢出
    let end_offset = offset.checked_add(struct_size)
        .ok_or_else(|| BofError::BoundsCheckFailed {
            offset,
            size: buffer.len(),
        })?;

    // 检查边界
    if end_offset > buffer.len() {
        return Err(BofError::BoundsCheckFailed {
            offset: end_offset,
            size: buffer.len(),
        });
    }

    // 检查对齐
    let ptr = buffer.as_ptr().add(offset);
    if (ptr as usize) % std::mem::align_of::<T>() != 0 {
        return Err(BofError::InvalidCoffFormat(
            format!("Misaligned structure at offset 0x{:X}", offset)
        ));
    }

    Ok(&*(ptr as *const T))
}

/// 安全地从缓冲区读取 packed 结构体（无对齐要求）
pub unsafe fn read_packed_struct<T>(buffer: &[u8], offset: usize) -> BofResult<T>
where
    T: Copy,
{
    let struct_size = std::mem::size_of::<T>();

    // 检查是否会溢出
    let end_offset = offset.checked_add(struct_size)
        .ok_or_else(|| BofError::BoundsCheckFailed {
            offset,
            size: buffer.len(),
        })?;

    // 检查边界
    if end_offset > buffer.len() {
        return Err(BofError::BoundsCheckFailed {
            offset: end_offset,
            size: buffer.len(),
        });
    }

    // 使用 read_unaligned 读取 packed 结构体
    let ptr = buffer.as_ptr().add(offset) as *const T;
    Ok(std::ptr::read_unaligned(ptr))
}

/// 安全地读取切片
pub unsafe fn read_slice<T>(buffer: &[u8], offset: usize, count: usize) -> BofResult<&[T]>
where
    T: Copy,
{
    let element_size = std::mem::size_of::<T>();

    // 检查 count * element_size 是否溢出
    let total_size = count.checked_mul(element_size)
        .ok_or_else(|| BofError::BoundsCheckFailed {
            offset,
            size: buffer.len(),
        })?;

    // 检查 offset + total_size 是否溢出
    let end_offset = offset.checked_add(total_size)
        .ok_or_else(|| BofError::BoundsCheckFailed {
            offset,
            size: buffer.len(),
        })?;

    // 检查边界
    if end_offset > buffer.len() {
        return Err(BofError::BoundsCheckFailed {
            offset: end_offset,
            size: buffer.len(),
        });
    }

    let ptr = buffer.as_ptr().add(offset) as *const T;
    Ok(std::slice::from_raw_parts(ptr, count))
}

/// 验证指针是否在有效范围内
pub unsafe fn validate_pointer(ptr: *const u8, base: usize, size: usize) -> BofResult<()> {
    let ptr_addr = ptr as usize;

    if ptr_addr < base || ptr_addr >= base + size {
        return Err(BofError::BoundsCheckFailed {
            offset: ptr_addr,
            size,
        });
    }

    Ok(())
}

/// 安全地计算指针偏移
pub unsafe fn safe_ptr_offset<T>(ptr: *const T, offset: isize, base: usize, size: usize) -> BofResult<*const T> {
    let element_size = std::mem::size_of::<T>() as isize;

    // 计算字节偏移
    let byte_offset = offset.checked_mul(element_size)
        .ok_or_else(|| BofError::BoundsCheckFailed {
            offset: offset as usize,
            size,
        })?;

    // 计算新地址
    let ptr_addr = ptr as isize;
    let new_addr = ptr_addr.checked_add(byte_offset)
        .ok_or_else(|| BofError::BoundsCheckFailed {
            offset: byte_offset as usize,
            size,
        })?;

    // 验证新地址在范围内
    if new_addr < base as isize || new_addr >= (base + size) as isize {
        return Err(BofError::BoundsCheckFailed {
            offset: new_addr as usize,
            size,
        });
    }

    Ok(new_addr as *const T)
}

/// 安全地复制内存
pub unsafe fn safe_copy_memory(
    dest: *mut u8,
    src: *const u8,
    count: usize,
    dest_base: usize,
    dest_size: usize,
) -> BofResult<()> {
    let dest_addr = dest as usize;

    // 检查目标地址在范围内
    if dest_addr < dest_base || dest_addr >= dest_base + dest_size {
        return Err(BofError::BoundsCheckFailed {
            offset: dest_addr,
            size: dest_size,
        });
    }

    // 检查复制后不会溢出
    let end_addr = dest_addr.checked_add(count)
        .ok_or_else(|| BofError::BoundsCheckFailed {
            offset: dest_addr,
            size: dest_size,
        })?;

    if end_addr > dest_base + dest_size {
        return Err(BofError::BoundsCheckFailed {
            offset: end_addr,
            size: dest_size,
        });
    }

    // 执行复制
    std::ptr::copy_nonoverlapping(src, dest, count);

    Ok(())
}

/// 验证 COFF 文件头
pub fn validate_coff_header(buffer: &[u8]) -> BofResult<()> {
    use std::mem::size_of;
    use super::bof::CoffFileHeader;

    // 最小大小检查
    let min_size = size_of::<CoffFileHeader>();
    if buffer.len() < min_size {
        return Err(BofError::FileTooSmall(buffer.len(), min_size));
    }

    Ok(())
}

/// 验证段表偏移
pub fn validate_section_table(
    buffer: &[u8],
    header_size: usize,
    section_count: u16,
) -> BofResult<()> {
    use std::mem::size_of;
    use super::bof::CoffSectionHeader;

    let section_header_size = size_of::<CoffSectionHeader>();

    // 计算段表总大小
    let table_size = (section_count as usize)
        .checked_mul(section_header_size)
        .ok_or_else(|| BofError::InvalidCoffFormat(
            "Section table size overflow".to_string()
        ))?;

    // 计算段表结束位置
    let table_end = header_size
        .checked_add(table_size)
        .ok_or_else(|| BofError::InvalidCoffFormat(
            "Section table offset overflow".to_string()
        ))?;

    // 检查是否超出缓冲区
    if table_end > buffer.len() {
        return Err(BofError::BoundsCheckFailed {
            offset: table_end,
            size: buffer.len(),
        });
    }

    Ok(())
}

/// 验证符号表偏移
pub fn validate_symbol_table(
    buffer: &[u8],
    symbol_table_offset: u32,
    symbol_count: u32,
) -> BofResult<()> {
    use std::mem::size_of;
    use super::bof::CoffSymbol;

    let symbol_size = size_of::<CoffSymbol>();

    // 计算符号表总大小
    let table_size = (symbol_count as usize)
        .checked_mul(symbol_size)
        .ok_or_else(|| BofError::InvalidCoffFormat(
            "Symbol table size overflow".to_string()
        ))?;

    // 计算符号表结束位置
    let table_end = (symbol_table_offset as usize)
        .checked_add(table_size)
        .ok_or_else(|| BofError::InvalidCoffFormat(
            "Symbol table offset overflow".to_string()
        ))?;

    // 检查是否超出缓冲区
    if table_end > buffer.len() {
        return Err(BofError::BoundsCheckFailed {
            offset: table_end,
            size: buffer.len(),
        });
    }

    Ok(())
}

/// 验证重定位表偏移
pub fn validate_relocation_table(
    buffer: &[u8],
    reloc_offset: u32,
    reloc_count: u16,
) -> BofResult<()> {
    use std::mem::size_of;
    use super::bof::CoffRelocation;

    let reloc_size = size_of::<CoffRelocation>();

    // 计算重定位表总大小
    let table_size = (reloc_count as usize)
        .checked_mul(reloc_size)
        .ok_or_else(|| BofError::InvalidCoffFormat(
            "Relocation table size overflow".to_string()
        ))?;

    // 计算重定位表结束位置
    let table_end = (reloc_offset as usize)
        .checked_add(table_size)
        .ok_or_else(|| BofError::InvalidCoffFormat(
            "Relocation table offset overflow".to_string()
        ))?;

    // 检查是否超出缓冲区
    if table_end > buffer.len() {
        return Err(BofError::BoundsCheckFailed {
            offset: table_end,
            size: buffer.len(),
        });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bounds_check() {
        let buffer = vec![0u8; 100];

        // 正常情况
        unsafe {
            let result = read_slice::<u8>(&buffer, 0, 50);
            assert!(result.is_ok());
        }

        // 越界
        unsafe {
            let result = read_slice::<u8>(&buffer, 50, 100);
            assert!(result.is_err());
        }

        // 溢出
        unsafe {
            let result = read_slice::<u8>(&buffer, usize::MAX, 1);
            assert!(result.is_err());
        }
    }

    #[test]
    fn test_validate_coff_header() {
        let buffer = vec![0u8; 20];
        assert!(validate_coff_header(&buffer).is_ok());

        let small_buffer = vec![0u8; 10];
        assert!(validate_coff_header(&small_buffer).is_err());
    }
}
