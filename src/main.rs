use eframe::{egui, CreationContext};
use egui::accesskit::Point;
use egui::Id;
use re_ui::UiExt;
use serialport::{available_ports, SerialPortType};
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use std::sync::mpsc::{Receiver, channel};

const TARGET_FRAME_RATE: usize = 60;
// Wordt gebruikt voor het scannen naar de Metalshare Hub
const KNOWN_MANUFACTURER: &str = "Espressif";
// Aantal sensoren die worden gebruikt
const NUM_SENSORS: usize = 8;

#[derive(Clone)]
struct ConnectionInfo {
    port_path: String,
    baudrate: u32,
}

impl ConnectionInfo {
    fn new(port_path: String, baudrate: u32) -> Self {
        ConnectionInfo {
            port_path,
            baudrate,
        }
    }
}

fn main() -> eframe::Result {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_app_id("metalstream")
            .with_decorations(!re_ui::CUSTOM_WINDOW_DECORATIONS)
            .with_fullsize_content_view(re_ui::FULLSIZE_CONTENT)
            .with_inner_size([800.0, 600.0])
            .with_title_shown(!re_ui::FULLSIZE_CONTENT)
            .with_titlebar_buttons_shown(!re_ui::CUSTOM_WINDOW_DECORATIONS)
            .with_titlebar_shown(!re_ui::FULLSIZE_CONTENT)
            .with_transparent(re_ui::CUSTOM_WINDOW_DECORATIONS),

        ..Default::default()
    };

    eframe::run_native(
        "Metalshare",
        native_options,
        Box::new(move |cc| {
            re_ui::apply_style_and_install_loaders(&cc.egui_ctx);
            Ok(Box::new(MyApp::new(cc)))
        }),
    )
}

#[derive(Debug, Clone)]
struct ParsedMessage {
    timestamp: String,
    command: String,
    fields: HashMap<String, String>,
}

// Formatter voor logs
impl std::fmt::Display for ParsedMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "[{}] {} {}",
            self.timestamp,
            self.command,
            self.fields
                .iter()
                .map(|(key, value)| format!("{}={}", key, value))
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}

#[derive(Default, Clone, Copy)]
pub struct Measurement {
    id: u8,
    connected: bool,
    value: u16,
}

// Globale applicatie state
pub struct GlobalState {
    is_connected: Arc<AtomicBool>,
    connection_info: Arc<Mutex<Option<ConnectionInfo>>>,
    logs: VecDeque<String>,
    serial_port_path: String,
    
    connection_states: [bool; 8],
    thread_spawned: bool,
    dimensions: Point,
    speed: f64,
    measurements: BTreeMap<u8, Measurement>,

    show_side_panel: bool,
}

impl Default for GlobalState {
    fn default() -> Self {
        Self {
            is_connected: Arc::new(AtomicBool::new(false)),
            connection_info: Arc::new(Mutex::new(None)),
            logs: VecDeque::new(),
            serial_port_path: String::new(),
            connection_states: [false; 8],
            thread_spawned: false,
            dimensions: Point::default(),
            speed: 0.0,
            measurements: BTreeMap::new(),
            show_side_panel: true,
        }
    }
}

struct MyApp {
    tree: egui_tiles::Tree<Tab>,
    state: GlobalState,
    log_receiver: Option<Receiver<ParsedMessage>>,
    visualization_tab: Arc<Mutex<VisualizationTab>>,
}

impl MyApp {
    fn new(cc: &CreationContext) -> Self {
        egui_extras::install_image_loaders(&cc.egui_ctx);
        re_ui::apply_style_and_install_loaders(&cc.egui_ctx);
        egui_material_icons::initialize(&cc.egui_ctx);

        let visualization_tab = Arc::new(Mutex::new(VisualizationTab::new()));

        let tabs: Vec<Tab> = vec![
            Arc::new(Mutex::new(ResultsTab)),
            visualization_tab.clone(),
            Arc::new(Mutex::new(LogsTab)),
        ];
        

        let tree = egui_tiles::Tree::new_vertical(Id::new("bla"), tabs);
        
        Self {
            tree,
            state: Default::default(),
            log_receiver: Default::default(),
            visualization_tab,
        }
    }

    fn spawn_serial_thread(&mut self) {
        let (sender, receiver) = channel();

        let port_path = Arc::new(self.state.serial_port_path.clone());
        let connection_info = self.state.connection_info.clone();
        let is_connected_clone = self.state.is_connected.clone();

        // Handel de seriele communicatie in een aparte thread om de GUI niet te blokkeren
        std::thread::spawn(move || {
            loop {
                let port_result = serialport::new(&*port_path, 115200)
                    .timeout(std::time::Duration::from_secs(1))
                    .open();

                match port_result {
                    Ok(mut port) => {
                        is_connected_clone.store(true, Ordering::Relaxed);
                        *connection_info.lock().unwrap() = Some(ConnectionInfo::new((*port_path).clone(), 115200));

                        let mut buffer = vec![0; 1024];
                        loop {
                            match port.read(&mut buffer) {
                                Ok(size) if size > 0 => {
                                    let received_chunk = String::from_utf8_lossy(&buffer[..size]).to_string();

                                    // Check of het bericht compleet is
                                    if received_chunk.contains('$') && received_chunk.contains('#') {
                                        if let Some(start) = received_chunk.find('$') {
                                            if let Some(end) = received_chunk[start..].find('#') {
                                                let full_message = &received_chunk[start..=start + end];

                                                // Split de berichten in delen
                                                let parts: Vec<&str> = full_message[1..full_message.len() - 1].split(':').collect();
                                                if parts.len() >= 2 {
                                                    let timestamp = parts[0].to_string();
                                                    let command = parts[1].to_string();

                                                    let mut fields = std::collections::HashMap::new();

                                                    for field in parts.iter().skip(2) {
                                                        if let Some((key, value)) = field.split_once('=') {
                                                            // Toevoegen aan het veld
                                                            fields.insert(key.to_string(), value.to_string());
                                                        }
                                                    }

                                                    // Maak een ParsedMessage
                                                    let parsed_message = ParsedMessage {
                                                        timestamp,
                                                        command,
                                                        fields,
                                                    };

                                                    sender.send(parsed_message).ok();
                                                }
                                            }
                                        }
                                    }
                                }
                                Ok(_) => {
                                    continue;
                                }
                                Err(err) => match err.kind() {
                                    // Negeer timeouts
                                    std::io::ErrorKind::TimedOut => {
                                        continue;
                                    }
                                    _ => {
                                        println!("Serial error {}", err);
                                        // Zet de verbinding naar false
                                        is_connected_clone.store(false, Ordering::Relaxed);
                                        break;
                                    }
                                },
                            }
                        }
                    }
                    Err(err) => {
                        println!("Error: handle serial thread error {}", err.to_string());
                        std::thread::sleep(Duration::from_secs(1));
                    }
                }
            }
        });

        self.log_receiver = Some(receiver);
    }
}

impl eframe::App for MyApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let is_connected = self.state.is_connected.load(Ordering::Relaxed);

        if !self.state.thread_spawned && self.state.serial_port_path.len() > 0 {
            self.spawn_serial_thread();
            self.state.thread_spawned = true;
        }

        if !is_connected {
            let ports = available_ports().unwrap();

            for port in ports {
                if let SerialPortType::UsbPort(usb) = &port.port_type {
                    if let Some(manufacturer) = &usb.manufacturer {
                        if manufacturer == KNOWN_MANUFACTURER {
                            self.state.serial_port_path = port.port_name;
                        }
                    }
                }
            }
        }
        
        if let Some(receiver) = &self.log_receiver {
        for log_message in receiver.try_iter() {
            self.state.logs.push_back(log_message.to_string());

            if self.state.logs.len() > 100 {
                self.state.logs.pop_front();
            }

            match log_message.command.as_str()
            {
                "SMS" => {
                    if let Some(id) = log_message.fields.get("ID") {
                        let index = id.parse::<usize>().unwrap()-1;

                        if let (Some(Ok(id)), Some(Ok(connected_parsed)), Some(Ok(value))) = (
                            log_message.fields.get("ID").map(|t| t.parse::<u8>()),
                            log_message.fields.get("C").map(|v| v.parse::<u8>()),
                            log_message.fields.get("V").map(|v| v.parse::<u16>()),
                        ) {
                            let connected = connected_parsed != 0;
                            self.state.connection_states[index] = connected;

                            let measurement = Measurement { id, connected, value };
                            self.state.measurements.insert(id, measurement);

                            let mut tab = self.visualization_tab.lock().unwrap();
                            tab.add_sensor_value(measurement.clone());

                        }
                    };
                },
                "MET" => {
                    println!("MET message: {:?}", log_message.fields);
                    if let (Some(Ok(width)), Some(Ok(length)), Some(Ok(speed))) = (
                        log_message.fields.get("W").map(|t| t.parse::<f64>()),
                        log_message.fields.get("L").map(|v| v.parse::<f64>()),
                        log_message.fields.get("S").map(|v| v.parse::<f64>()),
                    ) {
                        self.state.dimensions = Point::new(width, length);
                        self.state.speed = speed;
                        println!("{:?}", self.state.dimensions);
                    }
                },
                _ => println!("else"),
            }
        }
        }

        egui::TopBottomPanel::top("top_bar")
            .frame(re_ui::DesignTokens::top_panel_frame())
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    egui::menu::bar(ui, |ui| {
                        ui.menu_button("File", |ui| {
                            ui.add(egui::Button::new("Quit"))
                        });
                    });
    
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.medium_icon_toggle_button(
                            &re_ui::icons::LEFT_PANEL_TOGGLE,
                            &mut self.state.show_side_panel,
                        );
                    });    
                })
            });

        egui::TopBottomPanel::bottom("bottom_panel")
            .frame(re_ui::DesignTokens::bottom_panel_frame())
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if is_connected {
                        if let Some(ref connection_info) = self.state.connection_info.lock().unwrap().as_ref() {
                            if ui.button(egui::RichText::new(format!("{} {}", egui_material_icons::icons::ICON_POWER, connection_info.port_path)).size(10.0)).clicked() {
                                // Acties wanneer de knop wordt geklikt
                                println!("USB icon button clicked!");
                            }
                            let _ = ui.label(egui::RichText::new(connection_info.baudrate.to_string()).size(10.0));
                        }
                    } else {
                        ui.add(egui::Spinner::new());
                    }
                });
            });

        
        egui::SidePanel::left("left_panel")
            .width_range(150.0..=300.0)
            .resizable(true)
            .frame(egui::Frame {
                fill: ctx.style().visuals.panel_fill,
                inner_margin: egui::Margin::same(5.0),
                ..Default::default()
            })
            .show_animated(ctx, self.state.show_side_panel, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add(egui::Image::new(egui::include_image!("../assets/Logo_Full_White_Transparent.png")).max_width(100.0));
                });

                ui.horizontal_wrapped(|ui| {
                    ui.button("Start");
                    ui.button("Stop");
                    if ui.button("Calibrate").clicked() {

                    };
                });

                re_ui::list_item::list_item_scope(ui, "sensor_states", |ui| {
                ui.section_collapsing_header("Sensoren & Status")
                    .show(ui, |ui| {
                        for measurement_hash in self.state.measurements.iter() {
                            let (id, measurement) = measurement_hash;

                            ui.horizontal(|ui| {
                                ui.label(format!("Sensor S0{}", id));
                                
                                // Laat een icoontje zien op basis van de verbonden toestand
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if measurement.connected {
                                        ui.label(egui_material_icons::icon_text(egui_material_icons::icons::ICON_WIFI_TETHERING)
                                            .color(egui::Color32::WHITE)
                                            .size(16.0));
                                    } else {
                                        ui.label(egui_material_icons::icon_text(egui_material_icons::icons::ICON_WIFI_TETHERING_OFF)
                                            .size(16.0));
                                    }
                                });
                            });
                        }
                    });
                });
            });

            egui::CentralPanel::default().frame(egui::Frame {
                fill: ctx.style().visuals.panel_fill,
                ..Default::default()
            }).show(ctx, |ui| {
                self.tree.ui(&mut self.state, ui);
            });


            // Laat een los window zien als de hoofdapplicatie niet kan verbinden met het master board
            if !is_connected {
                self.state.speed = 0.0;
                self.state.dimensions = Point::new(0.0, 0.0);

                ctx.show_viewport_immediate(
                    egui::ViewportId::from_hash_of("connection_window"),
                    egui::ViewportBuilder::default()
                        .with_title("Verbinding mislukt")
                        .with_inner_size([300.0, 150.0]),
                    |ctx, _class| {
                        egui::CentralPanel::default().show(ctx, |ui| {    
                            ui.heading("Kan geen verbinding maken met de Metalstream Hub");
                            ui.label("De applicatie kan geen verbinding maken met het USB-apparaat. Controleer of het apparaat correct is aangesloten en probeer het opnieuw.");
                        });
                    },
                );
            }
            
            // Repaint TARGET_FRAME_RATE frames per seconde
            ctx.request_repaint_after(std::time::Duration::from_millis((1000/TARGET_FRAME_RATE).try_into().unwrap()));
    }
}


pub trait RenderableTab {
    fn title(&self) -> &str;
    fn ui(&mut self, ui: &mut egui::Ui, state: &mut GlobalState);
}

pub type Tab = Arc<Mutex<dyn RenderableTab>>;

pub struct LogsTab;

impl RenderableTab for LogsTab {
    fn title(&self) -> &str {
        "Logs"
    }

    fn ui(&mut self, ui: &mut egui::Ui, state: &mut GlobalState) {
    // Laat de seriele communicatie zien in een scrollable area.
    eframe::egui::ScrollArea::vertical()
    .auto_shrink(false)
    .show(ui, |ui| {
        for log in &state.logs {
            ui.label(egui::RichText::new(log).monospace());
        }
        ui.scroll_to_cursor(Some(egui::Align::BOTTOM));
    });
    }
}

pub struct ResultsTab;

impl RenderableTab for ResultsTab {
    fn title(&self) -> &str {
        "Results"
    }

    fn ui(&mut self, ui: &mut egui::Ui, state: &mut GlobalState) {
        // Geef de breedte, lengte en snelheid weer in de GUI.
        ui.label(format!("Width: {} mm", state.dimensions.x));
        ui.label(format!("Length: {} mm", state.dimensions.y));
        ui.label(format!("Snelheid: {} cm/s", state.speed));

        for i in 0..NUM_SENSORS {
            if let Some(measurement) = state.measurements.get(&((i+1) as u8)) {
                ui.label(format!("Sensor S0{}: {}", measurement.id, measurement.value));
            }
        }
    }
}

// // 250 => samples per seconds * (band lengte (cm) / band snelheid (cm/s)) => 10*(100\4)
const SAMPLE_BUF_SIZE: usize = 10*(100/4);

// even nummer
const VP_WIDTH: usize = 100;
const VP_HEIGHT: usize = 250;

const PX_PER_SENSOR: usize = VP_WIDTH / NUM_SENSORS;


pub struct VisualizationTab {
    cache: Cache,
    frame_counter: usize,
    sensor_buffer: VecDeque<Measurement>,
}

impl VisualizationTab {
    pub fn new() -> Self {
        Self {
            cache: Cache::default(),
            frame_counter: 0,
            sensor_buffer: VecDeque::with_capacity(SAMPLE_BUF_SIZE),
        }
    }

    pub fn add_sensor_value(&mut self, measurement: Measurement) {
        if self.sensor_buffer.len() >= SAMPLE_BUF_SIZE {
            self.sensor_buffer.pop_front();
        }
        self.sensor_buffer.push_back(measurement);
    }
}

impl RenderableTab for VisualizationTab {
    fn title(&self) -> &str {
        "Visualization"
    }

    fn ui(&mut self, ui: &mut egui::Ui, state: &mut GlobalState) {
        let size = egui::Vec2::new(VP_WIDTH as f32, VP_HEIGHT as f32);

        self.cache.resize(size);
        // self.cache.pixels.fill(egui::Color32::BLACK);

        for y in 1..VP_HEIGHT {
            let src_start = y * VP_WIDTH;
            let dest_start = (y - 1) * VP_WIDTH;
            self.cache.pixels.copy_within(src_start..src_start + VP_WIDTH, dest_start);
        }

        let mut new_row = vec![0.0; VP_WIDTH];

        for (sensor_idx, measurement) in state.measurements.values().enumerate() {
            if sensor_idx >= NUM_SENSORS {
                continue;
            }

            let intensity = (measurement.value as f32 / 2000.0).clamp(0.0, 1.0);

            let start_x = sensor_idx * PX_PER_SENSOR;
            let end_x = start_x + PX_PER_SENSOR;

            for x in start_x..end_x {
                let weight = 1.0 - ((x - start_x) as f32 / PX_PER_SENSOR as f32);
                new_row[x] += intensity * weight;
            }
        }

        // **Stap 4: Nieuwe rij pixels invullen onderaan**
        let last_row_idx = (VP_HEIGHT - 1) * VP_WIDTH;
        for (x, &intensity) in new_row.iter().enumerate() {
            self.cache.pixels[last_row_idx + x] = egui::Color32::from_gray((intensity * 255.0) as u8);
        }

        // **Stap 5: Texture genereren en renderen**
        let texture = egui::ColorImage {
            size: [VP_WIDTH, VP_HEIGHT],
            pixels: self.cache.pixels.clone(),
        };

        let handle = ui.ctx().load_texture("sensor", texture, Default::default());
        ui.add(egui::Image::new(&handle).fit_to_exact_size(size));
    }
}

impl egui_tiles::Behavior<Tab> for GlobalState {
    fn tab_title_for_pane(&mut self, tab: &Tab) -> egui::WidgetText {
        let locked_tab = tab.lock().unwrap();
        locked_tab.title().into()
    }

    fn pane_ui(
        &mut self,
        ui: &mut egui::Ui,
        _tile_id: egui_tiles::TileId,
        tab: &mut Tab,
    ) -> egui_tiles::UiResponse {
        egui::Frame::default().inner_margin(re_ui::DesignTokens::view_padding()).show(ui, |ui| {
            let mut locked_tab = tab.lock().unwrap();
            locked_tab.ui(ui, self);
        });

        Default::default()
    }

    fn tab_outline_stroke(
        &self,
        _visuals: &egui::Visuals,
        _tiles: &egui_tiles::Tiles<Tab>,
        _tile_id: egui_tiles::TileId,
        _tab_state: &egui_tiles::TabState,
    ) -> egui::Stroke {
        egui::Stroke::NONE
    }

    fn tab_bar_height(&self, _style: &egui::Style) -> f32 {
        re_ui::DesignTokens::title_bar_height()
    }

    fn simplification_options(&self) -> egui_tiles::SimplificationOptions {
        egui_tiles::SimplificationOptions {
            all_panes_must_have_tabs: true,
            ..Default::default()
        }
    }
}


#[derive(Default)]
struct Cache {
    pixels: Vec<egui::Color32>,
    size: egui::Vec2,
}

impl Cache {
    fn resize(&mut self, size: egui::Vec2) {
        if size == self.size {
            return;
        }

        self.pixels = Vec::new();
        self.pixels
            .resize((size.x*size.y) as usize, egui::Color32::default());
        self.size = size;
    }
}
