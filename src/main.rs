use eframe::wgpu::hal::vulkan::CommandEncoder;
use eframe::{egui, CreationContext};
use egui::accesskit::Point;
use egui::debug_text::print;
use egui::Order;
use egui_plot::{Line, Plot, PlotPoints};
use egui_tiles::Behavior;
use re_log::external::log::log_enabled;
use re_ui::{DesignTokens, UiExt};
use serialport::{available_ports, SerialPort, SerialPortBuilder, SerialPortType};
use std::collections::{HashMap, VecDeque};
use std::fmt::format;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use std::sync::mpsc::{Receiver, channel};


const KNOWN_MANUFACTURER: &str = "Espressif"; // Vervang door de daadwerkelijke fabrikant die je verwacht

#[derive(Clone)]
struct ConnectionInfo {
    port_path: String,
    baudrate: u32,
    is_connected: bool,
    // connection: Box<dyn SerialPort>,
}

impl ConnectionInfo {
    fn new(port_path: String, baudrate: u32) -> Self {
        ConnectionInfo {
            port_path,
            baudrate,
            is_connected: true,
            // connection
        }
    }
}

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_app_id("metalstream").with_inner_size([720.0, 480.0]),

        ..Default::default()
    };

    eframe::run_native(
        "Metalshare",
        options,
        Box::new(|cc| Ok(Box::new(MyApp::new(cc)))),
    )
}

#[derive(Debug, Clone)]
struct ParsedMessage {
    timestamp: String,
    command: String,
    fields: HashMap<String, String>,
}


#[derive(Clone, Copy, Debug)]
struct SensorSample {
    timestamp: u64,
    value: u16,
}

struct MyApp {
    tree: egui_tiles::Tree<TabType>,
    is_connected: Arc<AtomicBool>,
    connection_info: Arc<Mutex<Option<ConnectionInfo>>>,
    logs: VecDeque<String>,  // Optimaliseer logs-opslag
    // tx_channel: (std::sync::mpsc::Sender<ChannelMessage>, std::sync::mpsc::Receiver<ChannelMessage>),
    // rx_channel: (std::sync::mpsc::Sender<ChannelMessage>, std::sync::mpsc::Receiver<ChannelMessage>),
    log_receiver: Option<Receiver<ParsedMessage>>,  // Voor inkomende logs
    serial_port_path: String,
    
    connection_states: [bool; 8],
    sensor_values: [SensorSample; 8],
    thread_spawned: bool,
    dimensions: Point,
}

impl MyApp {
    fn new(cc: &CreationContext) -> Self {
        egui_extras::install_image_loaders(&cc.egui_ctx);
        re_ui::apply_style_and_install_loaders(&cc.egui_ctx);
        egui_material_icons::initialize(&cc.egui_ctx);

        let tabs: Vec<TabType> = vec![
            Arc::new(OverviewTab),
            Arc::new(SettingsTab),
        ];


        let tree = egui_tiles::Tree::new_tabs(egui::Id::new("tree"), tabs);

        Self {
            tree,
            is_connected: Default::default(),
            connection_info: Default::default(),
            // tx_channel: channel(),
            // rx_channel: channel(),
            logs: VecDeque::with_capacity(100),
            log_receiver: Default::default(),
            serial_port_path: Default::default(),
            connection_states: [false; 8],
            sensor_values: [SensorSample { timestamp: 0, value: 0};8],
            thread_spawned: false,
            dimensions: Default::default(),
        }
    }

    fn spawn_serial_thread(&mut self) {
        let (sender, receiver) = channel();
        // let (receiver, sender) = channel();

        let port_path = Arc::new(self.serial_port_path.clone());
        let connection_info = self.connection_info.clone();


        let is_connected_clone = self.is_connected.clone();
        std::thread::spawn(move || {
            loop {
                let port_result = serialport::new(&*port_path, 115200)
                    .timeout(std::time::Duration::from_secs(1))
                    .open();

                match port_result {
                    Ok(bla) => {
                        is_connected_clone.store(true, Ordering::Relaxed);

                        *connection_info.lock().unwrap() = Some(ConnectionInfo::new((*port_path).clone(), 115200));
                        
                        
                        let mut port = bla;
                        let mut buffer = vec![0; 1024]; 
                        loop {
                            match port.read(&mut buffer) {
                                Ok(size) if size > 0 => {
                                    let received_chunk = String::from_utf8_lossy(&buffer[..size]).to_string();

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
                                
                                                    sender
                                                    .send(parsed_message)
                                                    .ok();
                                                    
                                                }
                                            }
                                        }
                                    }
                                },
                                Ok(_) => {
                                    println!("Ok(_):")
                                },
                                Err(err) => {
                                    match err.kind() {
                                        std::io::ErrorKind::TimedOut => {
                                            continue;
                                        },
                                        _ => {
                                            println!("andere serial error {}", err);
                                            is_connected_clone.store(false, Ordering::Relaxed);
                                            break;
                                        }
                                    }
                                }, // Negeer timeouts
                            }
                        }

                        drop(port);
                    },
                    Err(err) => {
                        println!("Error: handle serial thread error {}", err.to_string());
                        std::thread::sleep(Duration::from_secs(1));
                    },
                }
            }
        });
    

        self.log_receiver = Some(receiver);
    }
}

impl eframe::App for MyApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let is_connected = self.is_connected.load(Ordering::Relaxed);

        if !self.thread_spawned && self.serial_port_path.len() > 0 {
            self.spawn_serial_thread();
            self.thread_spawned = true;
        }

        if !is_connected {
            let ports = available_ports().expect("bla");

            for port in ports {
                if let SerialPortType::UsbPort(usb) = &port.port_type {
                    if let Some(manufacturer) = &usb.manufacturer {
                        if manufacturer == KNOWN_MANUFACTURER {
                            println!("Manufacturer found, trying to connect {}", port.port_name);
                            self.serial_port_path = port.port_name;
                            // return try_connect(&port.port_name);
                        }
                    }
                }
            }
        }
        
        if let Some(receiver) = &self.log_receiver {
            for log_message in receiver.try_iter() {
                self.logs.push_back(format!("{} - command: {}", log_message.timestamp, log_message.command));

                if self.logs.len() > 100 {
                    self.logs.pop_front();
                }

                match log_message.command.as_str()
                {
                    "SMS" => {
                        if let Some(id) = log_message.fields.get("ID") {
                            let index = id.parse::<usize>().unwrap()-1;

                            if let Some(connected) = log_message.fields.get("C") {
                                self.connection_states[index] = connected.parse::<u8>().unwrap() != 0;
                            }

                            if let (Some(Ok(timestamp)), Some(Ok(value))) = (
                                log_message.fields.get("T").map(|t| t.parse::<u64>()),
                                log_message.fields.get("V").map(|v| v.parse::<u16>()),
                            ) {
                                self.sensor_values[index] = SensorSample { timestamp, value };
                            }
                        };
                    },
                    "MET" => {
                        if let (Some(Ok(width)), Some(Ok(length))) = (
                            log_message.fields.get("W").map(|t| t.parse::<f64>()),
                            log_message.fields.get("L").map(|v| v.parse::<f64>()),
                        ) {
                            self.dimensions = Point::new(width, length)
                        }

                        println!("{:?}", log_message.fields)

                    },
                    _ => println!("else"),
                }
            }
        }


        // let top_bar_style = ctx.top_bar_style(false);

        egui::TopBottomPanel::top("top_bar")
            .frame(re_ui::DesignTokens::top_panel_frame())
            // .exact_height(top_bar_style.height)
            .show(ctx, |ui| {
                #[cfg(not(target_arch = "wasm32"))]
                if !re_ui::NATIVE_WINDOW_BAR {
                    // Interact with background first, so that buttons in the top bar gets input priority
                    // (last added widget has priority for input).
                    let title_bar_response = ui.interact(
                        ui.max_rect(),
                        ui.id().with("background"),
                        egui::Sense::click(),
                    );
                    if title_bar_response.double_clicked() {
                        let maximized = ui.input(|i| i.viewport().maximized.unwrap_or(false));
                        ui.ctx()
                            .send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
                    } else if title_bar_response.is_pointer_button_down_on() {
                        // TODO(emilk): This should probably only run on `title_bar_response.drag_started_by(PointerButton::Primary)`,
                        // see https://github.com/emilk/egui/pull/4656
                        ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
                    }
                }

                egui::menu::bar(ui, |ui| {
                    // ui.set_height(top_bar_style.height);
                    // ui.add_space(top_bar_style.indent);

                    ui.menu_button("File", |ui| {
                        
                    });
                });
            });

        egui::TopBottomPanel::bottom("bottom_panel")
            .frame(re_ui::DesignTokens::bottom_panel_frame())
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if is_connected {
                        if let Some(ref connection_info) = self.connection_info.lock().unwrap().as_ref() {
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
            .show(ctx, |ui| {
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
                        for (i, connected) in self.connection_states.iter().enumerate() {
                            ui.horizontal(|ui| {
                                ui.label(format!("Sensor S0{}", i + 1)); // Link label
                                
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if *connected {
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
                tabs_ui(ui, &mut self.tree);

                ui.group(|ui| {
                    ui.label(format!("Width: {}mm", self.dimensions.x));
                    ui.label(format!("Length: {}mm", self.dimensions.y));
                });

                for (i, sample) in self.sensor_values.iter().enumerate() {
                    ui.group(|ui: &mut egui::Ui| {
                        ui.label(format!("S0{}: {}", i+1, sample.value.to_string()));
                    });
                }

                // eframe::egui::ScrollArea::vertical()
                // .auto_shrink(false)
                // .show(ui, |ui| {
                //     for log in &self.logs {
                //         // Fallback voor ongeldige logs
                //         ui.label(egui::RichText::new(log).monospace());
                //     }
                //     ui.scroll_to_cursor(Some(egui::Align::BOTTOM));
                // });
            });


            if !is_connected {
                ctx.show_viewport_immediate(
                    egui::ViewportId::from_hash_of("connection_window"),
                    egui::ViewportBuilder::default()
                        .with_title("Hardware Connection")
                        .with_inner_size([300.0, 150.0]),
                    |ctx, _class| {
                        egui::CentralPanel::default().show(ctx, |ui| {
                            // ui.label(&self.connection_message);
    
                            if ui.button("Opnieuw proberen").clicked() {
                                // self.show_connection_window = false;
                                // self.is_connected = true;
                            }
                        });
                    },
                );
            }
            
            ctx.request_repaint_after(std::time::Duration::from_millis(1000/60));
    }
}

fn tabs_ui(ui: &mut egui::Ui, tree: &mut egui_tiles::Tree<TabType>) {
    tree.ui(&mut MyTileTreeBehavior {}, ui);
}

pub trait Tab: Send + Sync {
    fn title(&self) -> String;
    fn ui(&self, ui: &mut egui::Ui);
}

pub struct OverviewTab;

impl Tab for OverviewTab {
    fn title(&self) -> String {
        "Overview".to_string()
    }

    fn ui(&self, ui: &mut egui::Ui) {
        // eframe::egui::ScrollArea::vertical()
        // .auto_shrink(false)
        // .show(ui, |ui| {
        //     for log in &self.logs {
        //         // Fallback voor ongeldige logs
        //         ui.label(egui::RichText::new(log).monospace());
        //     }
        //     ui.scroll_to_cursor(Some(egui::Align::BOTTOM));
        // });
    }
}

pub struct SettingsTab;

impl Tab for SettingsTab {
    fn title(&self) -> String {
        "Settings".to_string()
    }

    fn ui(&self, ui: &mut egui::Ui) {
        ui.heading("Settings");
        ui.label("Adjust your settings here.");
    }
}


pub type TabType = Arc<dyn Tab>;



// pub type Tab = i32;

struct MyTileTreeBehavior {}

impl egui_tiles::Behavior<TabType> for MyTileTreeBehavior {
    fn pane_ui(
        &mut self,
        ui: &mut egui::Ui,
        _tile_id: egui_tiles::TileId,
        pane: &mut TabType,
    ) -> egui_tiles::UiResponse {
        egui::Frame::default().inner_margin(4.0).show(ui, |ui| {
            pane.ui(ui);
        });

        Default::default()
    }

    fn tab_title_for_pane(&mut self, pane: &TabType) -> egui::WidgetText {
        pane.title().into()
    }

    // Styling:

    fn tab_outline_stroke(
        &self,
        _visuals: &egui::Visuals,
        _tiles: &egui_tiles::Tiles<TabType>,
        _tile_id: egui_tiles::TileId,
        _tab_state: &egui_tiles::TabState,
    ) -> egui::Stroke {
        egui::Stroke::NONE
    }

    /// The height of the bar holding tab titles.
    fn tab_bar_height(&self, _style: &egui::Style) -> f32 {
        re_ui::DesignTokens::title_bar_height()
    }

    /// What are the rules for simplifying the tree?
    fn simplification_options(&self) -> egui_tiles::SimplificationOptions {
        egui_tiles::SimplificationOptions {
            all_panes_must_have_tabs: true,
            ..Default::default()
        }
    }
}