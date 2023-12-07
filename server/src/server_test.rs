// TEST FILE 
#[cfg(test)]

// server functionality
use crate::start_server;

// basic functionality
use crate::translate_codon;
use crate::transcribe_dna;
use crate::translate;

// imports
use std::io::{self, Write, Read};
use std::net::TcpStream;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use std::thread;
use rand::seq::SliceRandom;


#[derive(Serialize, Deserialize, Debug)]
struct Request {
    request_type: String,
    data: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct Response {
    result_type: String,
    data: String,
    valid: bool,
}

fn fake_client(request_type: String, host_port: String, dnas: Vec<String>, sleep: u32) -> Vec<Response> {
    let mut results: Vec<Response> = Vec::new();
    // Connect to the server
    match TcpStream::connect(&host_port) {
        Ok(mut stream) => {
            let peer_addr = stream.local_addr().unwrap().to_string();
            ////////////////// Server Readiness //////////////////
            // wait for the first response from the server
            let mut buffer = [0; 524288];
            match stream.read(&mut buffer) {
                Ok(0) => {
                    // Zero-byte read, client has closed the connection
                    println!("[Client {:?}] Server not active.", peer_addr);
                    return results;
                }
                Ok(bytes_read) => {  // read successful ans deserialize
                    let response_data = &buffer[0..bytes_read];
                    let result: Result<Response, serde_json::Error> =
                        serde_json::from_slice(response_data);
                    match result {
                        Ok(response) => { // print response
                            println!("[Client {:?}] ✅: Server is ready for your requests.", peer_addr);
                        }
                        Err(e) => { // failed to deserialize
                            println!("[Client {:?}] Failed to deserialize Result: {:?}", peer_addr, e);
                            return results;
                        }
                    }
                }
                Err(_) => { // failed to read
                    println!("[Client {:?}] Failed to reach server.", peer_addr);
                    return results;
                }
            }

            ///////////////////////////////////////////////////////////////

            for dna in dnas {
                // crate request
                let request = Request {
                    request_type: request_type.clone(),
                    data: dna.clone(),
                };

                // serialize request and send to server
                let serialized = serde_json::to_string(&request).unwrap();
                // put here a sleep for sleep time
                thread::sleep(Duration::from_secs(sleep.into()));

                match stream.write(serialized.as_bytes()) {
                    Ok(_) => {}
                    // exit and cancel client
                    // print error message
                    Err(e) => {
                        println!("[Client {:?}] Failed to send request to server: {:?}", peer_addr, e);
                        break;
                    }
                }
                stream.flush().unwrap();

                // read response from server
                let mut buffer = [0; 524288];
                match stream.read(&mut buffer) {
                    Ok(0) => {
                        // Zero-byte read, client has closed the connection
                        println!("[Client {:?}] Client disconnected from server; shutting down.", peer_addr);
                        break;
                    }
                    Ok(bytes_read) => {  // read successful ans deserialize
                        let response_data = &buffer[0..bytes_read];
                        let result: Result<Response, serde_json::Error> = serde_json::from_slice(response_data);
                        // println!("Result: {:?}", result);
                        match result {
                            Ok(response) => { // print response
                                results.push(response);
                            }
                            Err(e) => { // failed to deserialize
                                println!("[Client {:?}] Failed to deserialize Result: {:?}", peer_addr, e);
                                break;
                            }
                        }
                    }
                    Err(_) => { // failed to read
                        println!("[Client {:?}] Failed to read Result from the server. You probably timed out or the server is unreachable; shutting down.", peer_addr);
                        break;
                    }
                }
            }
        }
        Err(_) => { // failed to connect to server
            println!("[Client] Could not connect to server.");
        }
    }
    return results;
}

fn generate_dna(num_triplets: usize, valid: bool) -> (String, String, String) {
    let mut dna = String::new();
    if valid {
        dna = "AUG".to_string();
    }
    let codons = vec![
        "UUU", "UUC", "UUA", "UUG", "CUU", "CUC", "CUA", "CUG",
        "AUU", "AUC", "AUA", "GUU", "GUC", "GUA", "GUG",
        "UCU", "UCC", "UCA", "UCG", "CCU", "CCC", "CCA", "CCG",
        "ACU", "ACC", "ACA", "ACG", "GCU", "GCC", "GCA", "GCG",
        "UAU", "UAC", "CAU", "CAC", "CAA", "CAG", "AAU", "AAC",
        "AAA", "AAG", "GAU", "GAC", "GAA", "GAG", "UGU", "UGC",
        "UGG", "CGU", "CGC", "CGA", "CGG", "AGA", "AGG", "AGU", "AGC",
        "GGU", "GGC", "GGA", "GGG"
    ];
    // choose num_triplets random codons and append to dna

    for _ in 0..num_triplets {
        let codon = codons.choose(&mut rand::thread_rng()).unwrap();
        dna.push_str(codon);
    }
    dna.push_str("UAA");

    // get transcription
    let transcription = transcribe_dna(&dna.clone());
    // get translation
    let translation = translate(&transcription.clone());

    return (dna, transcription, translation);
}

fn generate_dnas(triplet_nums: Vec<usize>) -> (Vec<String>, Vec<String>, Vec<String>) {
    let mut dnas: Vec<String> = Vec::new();
    let mut transcriptions: Vec<String> = Vec::new();
    let mut translations: Vec<String> = Vec::new();
    for num_triplets in triplet_nums {
        let (dna, transcription, translation) = generate_dna(num_triplets, true);
        dnas.push(dna);
        transcriptions.push(transcription);
        translations.push(translation);
    }
    return (dnas, transcriptions, translations);
}

mod tests {
    use std::{sync::{Arc, atomic::AtomicU16}, time::Instant};

    use crate::Capacities;

    use super::*;

    #[test]
    fn test_one_user_one_translation() {
        print!("one_user_one_translation\n");
        // start server in a new thread
        let _server_handle = thread::spawn(move || {
            let capacities = Arc::new(Capacities {
                max_request_workers: 2,
                max_work_workers: 4,
                max_work_workers_per_request: 2,
                minimum_codon_processing_unit: 3,
            });
            start_server(capacities, "8080".to_string(), true);
        });
        // wait for server to start
        thread::sleep(Duration::from_secs(1));
        // do test
        let (dnas, _, translations) = generate_dnas(vec![10]);
        let results = fake_client("-translate".to_string(), "localhost:8080".to_string(), dnas, 0);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].data, translations[0]);
    }

    #[test]
    fn test_one_user_one_transcription() {  
        print!("one_user_one_transcription\n");
        // start server in a new thread
        let _server_handle = thread::spawn(move || {
            let capacities = Arc::new(Capacities {
                max_request_workers: 8,
                max_work_workers: 4,
                max_work_workers_per_request: 2,
                minimum_codon_processing_unit: 1000,
            });
            start_server(capacities, "8080".to_string(), true);
        });
        // wait for server to start
        thread::sleep(Duration::from_secs(1));
        // do test
        let (dnas, transcriptions, _) = generate_dnas(vec![10]);
        let results = fake_client("-transcribe".to_string(), "localhost:8080".to_string(), dnas, 0);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].data, transcriptions[0]);
    }

    #[test]
    fn test_10_users_with_many_transcriptions() {
        // Start the server in a new thread
        let _server_handle = thread::spawn(move || {
            // Initialize capacities
            let capacities = Arc::new(Capacities {
                max_request_workers: 8,
                max_work_workers: 4,
                max_work_workers_per_request: 2,
                minimum_codon_processing_unit: 1000,
            });
            start_server(capacities, "8080".to_string(), true);
        });

        // Wait for the server to start
        thread::sleep(Duration::from_secs(1));

        // Start the users in respective threads
        let mut user_handles = Vec::new();
        
        // Assuming generate_dnas returns a tuple of vectors (dnas, transcriptions, translations)
        let (dnas, transcriptions, _) =
            generate_dnas(vec![500, 500, 500, 500, 500, 500, 500, 10000, 10000, 10000, 10000, 10000, 10000, 10000, 10000, 10000, 10000]);

        let before = Instant::now(); // Simple timing

        for _ in 0..10 {
            let dnas = dnas.clone();
            let handle = thread::spawn(move || {
                fake_client("-transcribe".to_string(), "localhost:8080".to_string(), dnas.clone(), 0)
            });
            user_handles.push(handle);
        }

        // Wait for users to finish
        for handle in user_handles {
            // Check the result
            let result = handle.join().unwrap();
            assert_eq!(result.len(), transcriptions.len());
            for i in 0..result.len() {
                // Assuming the response structure has a 'data' field
                assert_eq!(result[i].data, transcriptions[i]);
            }
        }

        let after = Instant::now();
        println!("10 users with 10 translations each took {:?}", after.duration_since(before));
    }

    #[test]
    fn test_10_users_with_many_translations() {
        // start server in a new thread
        let _server_handle = thread::spawn(move || {
            // init capacities
            let capacities = Arc::new(Capacities {
                max_request_workers: 8,
                max_work_workers: 4,
                max_work_workers_per_request: 2,
                minimum_codon_processing_unit: 1000,
            });
            start_server(capacities, "8080".to_string(), true);
        });
        // wait for server to start
        thread::sleep(Duration::from_secs(1));
        // start the users in respective threads
        let mut user_handles = Vec::new();
        let (dnas, _, translations) = generate_dnas(vec![500, 500, 500, 500, 500, 500, 500, 10000, 10000, 10000, 10000, 10000, 10000, 10000, 10000, 10000, 10000]);
        let before = Instant::now(); // simple timing
        for _ in 0..10 {
            let dnas = dnas.clone();
            let handle = thread::spawn(move || {
                fake_client("-translate".to_string(), "localhost:8080".to_string(), dnas.clone(), 0)
            });
            user_handles.push(handle);
        }
        // wait for users to finish
        for handle in user_handles {
            // check the result
            let result = handle.join().unwrap();
            assert!(result.len() == translations.len());
            for i in 0..result.len() {
                assert!(result[i].data == translations[i]);
            }
        }
        let after = Instant::now();
        println!("10 users with 10 translations each took {:?}", after.duration_since(before));
    }

    //client timeout test
    #[test]
    fn test_client_terminated_due_to_timeout() {
        // write a test but dont send any data to server just sleep and then try to write to server
        print!("one_user_one_transcription\n");
        // start server in a new thread
        let _server_handle = thread::spawn(move || {
            let capacities = Arc::new(Capacities {
                max_request_workers: 1,
                max_work_workers: 2,
                max_work_workers_per_request: 2,
                minimum_codon_processing_unit: 1,
            });
            start_server(capacities, "8080".to_string() , true);
        });
        // wait for server to start
        thread::sleep(Duration::from_secs(1));
        // do test
        let (dnas, transcriptions, _) = generate_dnas(vec![1]);
        let results = fake_client("-transcribe".to_string(), "localhost:8080".to_string(), dnas, 11);

        assert_eq!(results.len(), 0);
        // check if client still running or timeout message was printed to console
    }

}