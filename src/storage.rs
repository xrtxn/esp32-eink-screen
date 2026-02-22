use core::cell::RefCell;

use embassy_embedded_hal::adapter::BlockingAsync;
use esp_storage::FlashStorage;
use static_cell::StaticCell;

const NVS_STORAGE_START: u32 = 0x9000;
const NVS_STORAGE_SIZE: u32 = 0x6000;
const NVS_RANGE: core::ops::Range<u32> = NVS_STORAGE_START..NVS_STORAGE_START + NVS_STORAGE_SIZE;

const CONFIG_KEY: u8 = 1;

static FLASH: StaticCell<RefCell<FlashStorage<'static>>> = StaticCell::new();

pub(crate) fn init_flash(flash: FlashStorage<'static>) -> &'static RefCell<FlashStorage<'static>> {
    FLASH.init(RefCell::new(flash))
}

#[derive(serde::Serialize, serde::Deserialize, askama::Template, Debug)]
#[template(path = "index.html")]
pub struct NvsConfig {
    pub ssid: heapless::String<32>,
    pub password: heapless::String<32>,
    pub caldav: Caldav,
}

impl Default for NvsConfig {
    fn default() -> Self {
        Self {
            ssid: Default::default(),
            password: Default::default(),
            caldav: Default::default(),
        }
    }
}

impl sequential_storage::map::PostcardValue<'_> for NvsConfig {}

#[derive(serde::Serialize, serde::Deserialize, Debug)]
pub struct Caldav {
    pub url: heapless::String<128>,
    pub username: heapless::String<32>,
    pub password: heapless::String<32>,
}

impl Default for Caldav {
    fn default() -> Self {
        Self {
            url: Default::default(),
            username: Default::default(),
            password: Default::default(),
        }
    }
}

pub(crate) async fn read_config(flash_cell: &RefCell<FlashStorage<'static>>) -> Option<NvsConfig> {
    let mut borrow = flash_cell.borrow_mut();
    let mut data_buffer = [0u8; 256];

    let async_flash = BlockingAsync::new(&mut *borrow);

    let mut l = sequential_storage::map::MapStorage::<u8, _, _>::new(
        async_flash,
        const { sequential_storage::map::MapConfig::new(NVS_RANGE) },
        sequential_storage::cache::NoCache::new(),
    );

    let nvs_config = l
        .fetch_item::<NvsConfig>(&mut data_buffer, &CONFIG_KEY)
        .await
        .ok()
        .map(|item| item.unwrap());

    log::info!("Read config: {:?}", nvs_config);
    nvs_config
}

pub(crate) async fn write_config(flash_cell: &RefCell<FlashStorage<'static>>, config: NvsConfig) {
    let mut borrow = flash_cell.borrow_mut();

    let async_flash = BlockingAsync::new(&mut *borrow);

    let mut data_buffer = [0u8; 256];

    let mut serialized_buf = [0u8; 128];
    let _ = postcard::to_slice(&config, &mut serialized_buf).expect("Failed to serialize config");

    let mut l = sequential_storage::map::MapStorage::<u8, _, _>::new(
        async_flash,
        const { sequential_storage::map::MapConfig::new(NVS_RANGE) },
        sequential_storage::cache::NoCache::new(),
    );

    l.store_item(&mut data_buffer, &CONFIG_KEY, &config)
        .await
        .unwrap();

    log::info!("Config written to flash");
}
