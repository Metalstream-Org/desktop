use eframe::wgpu::hal::vulkan::CommandEncoder;
use eframe::{egui, CreationContext};
use egui::Order;
use re_log::external::log::log_enabled;
use re_ui::UiExt;
use serialport::{available_ports, SerialPort, SerialPortBuilder, SerialPortType};
use std::collections::VecDeque;
use std::fmt::format;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use std::sync::mpsc::{Receiver, channel};

// <start_byte><command><payload_length><payload><checksum><end_byte>


// _MEAS


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

enum ChannelMessage {
    Data(ParsedMessage),
    Error(String),
    Connected,
}

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_app_id("metalstream").with_inner_size([720.0, 480.0]),

        ..Default::default()
    };

    eframe::run_native(
        "Hardware Connection Example",
        options,
        Box::new(|cc| Ok(Box::new(MyApp::new(cc)))),
    )
}

#[derive(Debug, Clone)]
struct ParsedMessage {
    timestamp: String,
    command: String,
    fields: std::collections::HashMap<String, String>,
}

struct MyApp {
    is_connected: bool,
    show_connection_window: bool,
    connection_info: Option<ConnectionInfo>,
    logs: VecDeque<String>,  // Optimaliseer logs-opslag
    // tx_channel: (std::sync::mpsc::Sender<ChannelMessage>, std::sync::mpsc::Receiver<ChannelMessage>),
    // rx_channel: (std::sync::mpsc::Sender<ChannelMessage>, std::sync::mpsc::Receiver<ChannelMessage>),
    log_receiver: Option<Receiver<ChannelMessage>>,  // Voor inkomende logs
    serial_port_path: String,
    
    connection_states: [bool; 8],
    thread_spawned: bool,

}

impl MyApp {
    fn new(cc: &CreationContext) -> Self {
        egui_extras::install_image_loaders(&cc.egui_ctx);
        re_ui::apply_style_and_install_loaders(&cc.egui_ctx);
        egui_material_icons::initialize(&cc.egui_ctx);

        Self {
            is_connected: Default::default(),
            show_connection_window: Default::default(),
            connection_info: Default::default(),
            // tx_channel: channel(),
            // rx_channel: channel(),
            logs: VecDeque::with_capacity(100),
            log_receiver: Default::default(),
            serial_port_path: Default::default(),
            connection_states: [false; 8],
            thread_spawned: false,
        }
    }

    fn spawn_serial_thread(&mut self) {
        let (sender, receiver) = channel();
        // let (receiver, sender) = channel();

        let port_path = Arc::new(self.serial_port_path.clone());
        let connection_info = Arc::new(Mutex::new(None::<ConnectionInfo>));
        let connection_info_clone = Arc::clone(&connection_info);

        std::thread::spawn(move || {
            loop {
                let port_result = serialport::new(&*port_path, 115200)
                    .timeout(std::time::Duration::from_secs(1))
                    .open();

                match port_result {
                    Ok(bla) => {
                        *connection_info_clone.lock().unwrap() = Some(ConnectionInfo::new((*port_path).clone(), 115200));
                        sender.send(ChannelMessage::Connected);
                        
                        
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
                                
                                                    // Verstuur de parsed message via het kanaal
                                                    sender
                                                        .send(ChannelMessage::Data(parsed_message))
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
                                            sender.send(ChannelMessage::Error(err.to_string())).ok();
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
        self.connection_info = connection_info.lock().unwrap().clone();
    }
}

impl eframe::App for MyApp {
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        // TODO: drop port
        // if let Some(connection_info) = self.connection_info.take() {
        //     drop(connection_info.connection);
        // }
    }
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if !self.thread_spawned && self.serial_port_path.len() > 0 {
            self.spawn_serial_thread();
            self.thread_spawned = true;
        }

        if !self.is_connected {
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
            for log in receiver.try_iter() {
                match log {
                    ChannelMessage::Data(log_message) => {
                        self.logs.push_back(format!("{} - command: {}", log_message.timestamp, log_message.command));

                        if self.logs.len() > 100 {
                            self.logs.pop_front();
                        }
                        
                        if let Some(id) = log_message.fields.get("ID") {
                            if let Some(connected) = log_message.fields.get("C") {
                                self.connection_states[id.parse::<usize>().unwrap()-1] = connected.parse::<u8>().unwrap() != 0;
                            }
                        };
                    }
                    ChannelMessage::Error(bla) => {
                        println!("Error bla: {}", bla);
                        self.is_connected = false;
                    },
                    ChannelMessage::Connected => {
                        self.is_connected = true;
                    }
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
                if let Some(ref connection_info) = self.connection_info {
                    ui.horizontal(|ui| {
                        if ui.button(egui::RichText::new(format!("{} {}", egui_material_icons::icons::ICON_POWER, connection_info.port_path)).size(10.0)).clicked() {
                            // Acties wanneer de knop wordt geklikt
                            println!("USB icon button clicked!");
                        }
                        let _ = ui.label(egui::RichText::new(connection_info.baudrate.to_string()).size(10.0));
                    });
                }
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
                    ui.button("Calibrate");
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
                inner_margin: egui::Margin::same(5.0),
                ..Default::default()
            }).show(ctx, |ui| {
                ui.heading("Dit is de root applicatie.");

                eframe::egui::ScrollArea::vertical()
                .auto_shrink(false)
                .show(ui, |ui| {
                    for log in &self.logs {
                        // Fallback voor ongeldige logs
                        ui.label(egui::RichText::new(log).monospace());
                    }
                    ui.scroll_to_cursor(Some(egui::Align::BOTTOM));
                });

                if let Some(ref mut connection_info) = self.connection_info {
                    ui.label(format!("Verbonden met: {}", connection_info.port_path));
                    ui.label(format!("Baudrate: {}", connection_info.baudrate));
                }
            });


            if self.show_connection_window {
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
    
                            if ui.button("Sluiten").clicked() {
                                self.show_connection_window = false
                            }
                        });
                    },
                );
            }

            ctx.request_repaint_after(std::time::Duration::from_millis(1000/60));
    }
}