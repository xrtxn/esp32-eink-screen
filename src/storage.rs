#[cfg(target_arch = "xtensa")]
use alloc::{string::String, vec::Vec};
#[cfg(not(target_arch = "xtensa"))]
use std::{string::String, vec::Vec};

#[derive(thiserror::Error, picoserve::response::ErrorWithStatusCode, Debug)]
#[status_code(INTERNAL_SERVER_ERROR)]
pub enum StorageError {
    #[error("Failed to read NVS")]
    ReadError,
}

#[derive(serde::Serialize, serde::Deserialize, Default, Debug, Clone)]
pub struct NvsConfig {
    pub wifi: Option<WifiCreds>,
    pub caldav: Option<CaldavCreds>,
    pub display: Option<DisplayConfig>,
}

impl NvsConfig {
    pub fn new(wifi: Option<WifiCreds>) -> Self {
        Self {
            wifi,
            caldav: None,
            display: None,
        }
    }
}

#[cfg_attr(feature = "defmt", derive(crate::defmt::Format))]
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct WifiCreds {
    pub ssid: heapless::String<32>,
    pub password: heapless::String<32>,
}

impl WifiCreds {
    pub fn new(ssid: &str, password: &str) -> Self {
        Self {
            ssid: heapless::String::try_from(ssid).unwrap(),
            password: heapless::String::try_from(password).unwrap(),
        }
    }
}

#[cfg_attr(feature = "defmt", derive(crate::defmt::Format))]
#[derive(serde::Serialize, serde::Deserialize, Default, Debug, Clone)]
pub struct CaldavCreds {
    pub url: heapless::String<128>,
    pub username: heapless::String<32>,
    pub password: heapless::String<32>,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct DisplayConfig {
    pub displayed_hours: u8,
    pub calendars: Vec<String>,
    #[serde(default)]
    pub next_n_hours_only: bool,
}

impl Default for DisplayConfig {
    fn default() -> Self {
        Self {
            displayed_hours: 18,
            calendars: Vec::new(),
            next_n_hours_only: false,
        }
    }
}

#[cfg(not(target_arch = "xtensa"))]
pub use not_xtensa::*;
#[cfg(target_arch = "xtensa")]
pub use xtensa::*;

#[cfg(target_arch = "xtensa")]
mod xtensa {
    use embassy_embedded_hal::adapter::BlockingAsync;
    use embassy_sync::blocking_mutex::raw::NoopRawMutex;
    use embassy_sync::mutex::Mutex;
    pub use esp_storage::FlashStorage;
    use static_cell::StaticCell;

    use super::NvsConfig;

    const NVS_STORAGE_START: u32 = 0x9000;
    const NVS_STORAGE_SIZE: u32 = 0x6000;
    const NVS_RANGE: core::ops::Range<u32> =
        NVS_STORAGE_START..NVS_STORAGE_START + NVS_STORAGE_SIZE;

    const CONFIG_KEY: u8 = 1;

    static FLASH: StaticCell<Mutex<NoopRawMutex, FlashStorage<'static>>> = StaticCell::new();

    pub(crate) fn init_flash(
        flash: FlashStorage<'static>,
    ) -> &'static Mutex<NoopRawMutex, FlashStorage<'static>> {
        FLASH.init(Mutex::new(flash))
    }

    impl sequential_storage::map::PostcardValue<'_> for NvsConfig {}

    pub(crate) async fn read_config(
        flash_cell: &Mutex<NoopRawMutex, FlashStorage<'static>>,
    ) -> Option<NvsConfig> {
        let mut borrow = flash_cell.lock().await;
        let mut data_buffer = [0u8; 3000];

        let async_flash = BlockingAsync::new(&mut *borrow);

        let mut ms = sequential_storage::map::MapStorage::<u8, _, _>::new(
            async_flash,
            const { sequential_storage::map::MapConfig::new(NVS_RANGE) },
            sequential_storage::cache::NoCache::new(),
        );

        let nvs_config = ms
            .fetch_item::<NvsConfig>(&mut data_buffer, &CONFIG_KEY)
            .await
            .ok()
            .flatten();

        nvs_config
    }

    pub(crate) async fn write_config(
        flash_cell: &Mutex<NoopRawMutex, FlashStorage<'static>>,
        config: NvsConfig,
    ) {
        let mut borrow = flash_cell.lock().await;

        let async_flash = BlockingAsync::new(&mut *borrow);

        let mut data_buffer = [0u8; 2048];

        let mut l = sequential_storage::map::MapStorage::<u8, _, _>::new(
            async_flash,
            const { sequential_storage::map::MapConfig::new(NVS_RANGE) },
            sequential_storage::cache::NoCache::new(),
        );

        l.store_item(&mut data_buffer, &CONFIG_KEY, &config)
            .await
            .unwrap();

        crate::defmt::info!("Config written to flash");
    }
}

#[cfg(not(target_arch = "xtensa"))]
mod not_xtensa {
    use super::*;

    pub async fn read_config() -> Option<NvsConfig> {
        Some(NvsConfig::default())
    }

    pub async fn write_config(config: NvsConfig) {
        crate::defmt::info!(
            "Mock writing config: {:?}",
            crate::defmt::Debug2Format(&config)
        );
    }
}
