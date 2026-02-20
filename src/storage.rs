#[derive(serde::Serialize, serde::Deserialize, Debug)]
pub struct NvsConfig<'a> {
    pub ssid: &'a str,
    pub password: &'a str,
}

#[derive(serde::Serialize, serde::Deserialize, Debug)]
pub struct Caldav<'a> {
    pub url: &'a str,
    pub username: &'a str,
    pub password: &'a str,
}

pub(crate) fn test_flash(flash_prp: esp_hal::peripherals::FLASH<'_>, config: NvsConfig) {
    let _flash = esp_storage::FlashStorage::new(flash_prp);
    let dest = 0x9000;

    let data: u32 = 0xDEADBEEF;

    let data_ptr = &data as *const u32;
    let len = core::mem::size_of::<u32>() as u32;
    unsafe { esp_storage::ll::spiflash_write(dest, data_ptr, len) }.unwrap();

    let mut read_val = 0_u32;
    let read_buf = &mut read_val as *mut u32;

    unsafe { esp_storage::ll::spiflash_read(dest, read_buf, len) }.unwrap();

    log::info!("Read data: {:#X}", read_val);
}
