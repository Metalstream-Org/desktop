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


const TARGET_FRAME_RATE: usize = 60;
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


#[derive(Default, Clone, Copy, Debug)]
struct SensorSample {
    timestamp: u64,
    value: u16,
}

#[derive(Default)]
struct GlobalState {
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
    speed: f64,
}

struct MyApp {
    state: GlobalState,
}

impl MyApp {
    fn new(cc: &CreationContext) -> Self {
        egui_extras::install_image_loaders(&cc.egui_ctx);
        re_ui::apply_style_and_install_loaders(&cc.egui_ctx);
        egui_material_icons::initialize(&cc.egui_ctx);

        Self {
            state: Default::default(),
        }
    }

    fn spawn_serial_thread(&mut self) {
        let (sender, receiver) = channel();
        // let (receiver, sender) = channel();

        let port_path = Arc::new(self.state.serial_port_path.clone());
        let connection_info = self.state.connection_info.clone();


        let is_connected_clone = self.state.is_connected.clone();
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
    

        self.state.log_receiver = Some(receiver);
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
            let ports = available_ports().expect("bla");

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
        
        if let Some(receiver) = &self.state.log_receiver {
            for log_message in receiver.try_iter() {
                self.state.logs.push_back(format!("{} - command: {}", log_message.timestamp, log_message.command));

                if self.state.logs.len() > 100 {
                    self.state.logs.pop_front();
                }

                match log_message.command.as_str()
                {
                    "SMS" => {
                        if let Some(id) = log_message.fields.get("ID") {
                            let index = id.parse::<usize>().unwrap()-1;

                            if let Some(connected) = log_message.fields.get("C") {
                                self.state.connection_states[index] = connected.parse::<u8>().unwrap() != 0;
                            }

                            if let (Some(Ok(timestamp)), Some(Ok(value))) = (
                                log_message.fields.get("T").map(|t| t.parse::<u64>()),
                                log_message.fields.get("V").map(|v| v.parse::<u16>()),
                            ) {
                                self.state.sensor_values[index] = SensorSample { timestamp, value };
                            }
                        };
                    },
                    "MET" => {
                        if let (Some(Ok(width)), Some(Ok(length)), Some(Ok(speed))) = (
                            log_message.fields.get("W").map(|t| t.parse::<f64>()),
                            log_message.fields.get("L").map(|v| v.parse::<f64>()),
                            log_message.fields.get("S").map(|v| v.parse::<f64>()),
                        ) {
                            self.state.dimensions = Point::new(width, length);
                            self.state.speed = speed;
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
                        for (i, connected) in self.state.connection_states.iter().enumerate() {
                            ui.horizontal(|ui| {
                                ui.label(format!("Sensor S0{}", i + 1));
                                
                                // Laat een icoontje zien op basis van de verbonden toestand
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

                // Geef de breedte, lengte en snelheid weer in de GUI.
                ui.group(|ui| {
                    ui.label(format!("Width: {}mm", self.state.dimensions.x));
                    ui.label(format!("Length: {}mm", self.state.dimensions.y));
                    ui.label(format!("Snelheid: {}mm", self.state.speed));
                });

                for (i, sample) in self.state.sensor_values.iter().enumerate() {
                    ui.group(|ui: &mut egui::Ui| {
                        ui.label(format!("S0{}: {}", i+1, sample.value.to_string()));
                    });
                }

                // Laat de seriele communicatie zien in een scrollable area.
                eframe::egui::ScrollArea::vertical()
                .auto_shrink(false)
                .show(ui, |ui| {
                    for log in &self.state.logs {
                        ui.label(egui::RichText::new(log).monospace());
                    }
                    ui.scroll_to_cursor(Some(egui::Align::BOTTOM));
                });
            });


            // Laat een los window zien als de hoofdapplicatie niet kan verbinden met het master board
            if !is_connected {
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