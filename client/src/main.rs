///////////////////////////////////////////////////////////
/////////////// PCLT - 2.project - RUST ///////////////////
//                     Client                            //
// The client binary take as CMD line arguments:         //
//      the server address and port and a flag to        //
//      determine whether it will perform                //
//                      -transcription                   //
//                      -translation                     //
// To launch the client, the command should be:          //
//          ./client -transcribe host:port               //
//              or                                       //                                              
//          ./client -translate host:port                //
// The client will then read from standard input         //
// the DNA sequence.                                     //
///////////////////////////////////////////////////////////

// imports
use std::io::{self, Write, Read};
use std::net::TcpStream;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use std::thread;

///////////////////// Useful Structs /////////////////////

// Request struct to send to the server
#[derive(Serialize, Deserialize, Debug)]
struct Request {
    request_type: String,
    data: String,
}

// Response struct to receive from the server   
#[derive(Serialize, Deserialize, Debug)]
struct Response {
    result_type: String,
    data: String,
    valid: bool,
}

///////////////////// Heartbeat Thread /////////////////////

// The heartbeat thread will check the connection status every 5 seconds
// If the connection is lost, the client will exit
fn heartbeat_thread(host_port: String, mut stream: TcpStream) {
    loop {
        // Check the connection status every 5 seconds
        thread::sleep(Duration::from_secs(5));

        // Set a read timeout for 1 millisecond
        stream.set_read_timeout(Some(Duration::from_millis(1))).expect("Failed to set read timeout");

        // Try to read 1 byte from the stream
        let mut buffer = [0; 1];
        match stream.read_exact(&mut buffer) {
            Ok(_) => {
                // Connection is still alive
                continue;
            }
            // on linux
            Err(ref err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                // Timeout expired with no data read, but connection is still alive
                continue;
            }
            // on windows
            Err(ref err) if err.kind() == std::io::ErrorKind::TimedOut => {
                // Timeout expired with no data read, but connection is still alive
                continue;
            }
            Err(_) => {
                // Other errors, connection might be closed
                println!("Lost connection to server at {}, You probably timed out or the server is unreachable.", host_port);
                // exit and cancel client
                std::process::exit(1);
            }
        }
    }
}

///////////////////// Main /////////////////////

fn main() {
    // get command line arguments
    let args: Vec<String> = std::env::args().collect();
    let mut request_type = String::new();
    let mut host_port = String::new();

    // check if user input is valid
    if args.len() != 3 {
        // print usage message if not valid
        println!("Usage: ./client -transcribe host:port");
        println!("Usage: ./client -translate host:port");
        std::process::exit(1);
    }

    // Parse the command line arguments
    request_type = args[1].clone();
    host_port = args[2].clone();

    // Connect to the server
    match TcpStream::connect(&host_port) {
        Ok(mut stream) => {
            // Create a copy of the stream for the heartbeat thread
            let copy_stream = stream.try_clone().unwrap();
            let host_port_heartbeat = host_port.clone();

            ////////////////// Server Readiness //////////////////
            // wait for the first response from the server
            let mut buffer = [0; 524288];
            println!("--------------------------------------------------");
            match stream.read(&mut buffer) {
                Ok(0) => {
                    // Zero-byte read, client has closed the connection
                    println!("Server not active.");
                    return;
                }
                Ok(bytes_read) => {  // read successful ans deserialize
                    let response_data = &buffer[0..bytes_read];
                    let result: Result<Response, serde_json::Error> =
                        serde_json::from_slice(response_data);
                    match result {
                        Ok(response) => { // print response
                            
                            println!("\n{}\n", response.data);
                            
                        }
                        Err(e) => { // failed to deserialize
                            println!("Failed to deserialize Result: {:?}", e);
                            return;
                        }
                    }
                    
                }
                Err(_) => { // failed to read
                    println!("Failed to reach server.");
                    return;
                }
            }
            println!("--------------------------------------------------");

            ///////////////////////////////////////////////////////////////

            let _heartbeat_handle = thread::spawn(move || {
                // start the heartbeat thread
                heartbeat_thread(host_port_heartbeat, copy_stream);
            });

            loop {
                // ask for input
                println!("Input DNA sequences split using Enter (10 s to timeout):");

                // read input and trim new line character
                let mut dna = String::new();
                io::stdin().read_line(&mut dna).expect("Failed to read line");
                let dna = dna.trim().to_string();

                if dna.is_empty() {
                    println!("DNA sequence cannot be empty.");
                    continue; // restart loop
                }

                // crate request
                let request = Request {
                    request_type: request_type.clone(),
                    data: dna.clone(),
                };

                // serialize request and send to server
                let serialized = serde_json::to_string(&request).unwrap();

                match stream.write(serialized.as_bytes()) {
                    Ok(_) => {}
                    Err(e) => {
                        // failed to send request
                        println!("Failed to send request to server: {:?}", e);
                        break;
                    }
                }
                stream.flush().unwrap();

                // read response from server
                let mut buffer = [0; 524288];
                println!("\n--------------------------------------------------");
                match stream.read(&mut buffer) {
                    Ok(0) => {
                        // Zero-byte read, client has closed the connection
                        println!("Client disconnected from server or timed out; shutting down.");
                        break;
                    }
                    Ok(bytes_read) => {  // read successful ans deserialize
                        let response_data = &buffer[0..bytes_read];
                        let result: Result<Response, serde_json::Error> =
                            serde_json::from_slice(response_data);
                        match result {
                            Ok(response) => { // print response
                                println!("Operation: {}\nResult   : {}\nValidity : {}", response.result_type, response.data, response.valid);
                            }
                            Err(e) => { // failed to deserialize
                                println!("Failed to deserialize Result: {:?}", e);
                                break;
                            }
                        }
                    }
                    Err(_) => { // failed to read
                        println!("Failed to read Result from the server. You probably timed out or the server is unreachable; shutting down.");
                        break;
                    }
                }
                println!("--------------------------------------------------\n");
            }
        }
        Err(_) => { // failed to connect to server
            println!("Could not connect to server.");
        }
    }
}
