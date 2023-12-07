///////////////////////////////////////////////////////////
/////////////// PCLT - 2. project - RUST //////////////////
//                   Server                              //
// The server receives requests from one or more clients,//
// performs the corresponding transcription or           // 
// translation (and validation) request and answers      //
// back with the reply.                                  //
// To launch the server, the command is:                 //
//                                    ./server port      //
///////////////////////////////////////////////////////////

// test module
mod server_test;

// imports
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::AtomicBool;
use std::thread;
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex, Condvar, RwLock};
use regex::Regex;
use std::time::{Duration, Instant}; 

// based on how many cpus we have
// we might want to have more or less workers
use num_cpus;

///////////////////// Useful Structs /////////////////////

// Requests contain a request type (translation or transcription) and the 
// data to be worked on
#[derive(Serialize, Deserialize, Debug)]
struct Request {
    request_type: String,
    data: String,
}

// Struct for a server-response to the client
#[derive(Serialize, Deserialize, Debug)]
struct Response {
    result_type: String,
    data: String,
    valid: bool,
}

// Workers operate on lists of tasks
struct Task {
    request: String,
    finished: Arc<(Mutex<bool>, Condvar)>,
    result: Arc<Mutex<String>>,
    translate: bool,
    client: String,
    valid: AtomicBool // validity could be set to false, if the client disconnects
    // so that no unnecessary work is done. In such a scenario every request handler
    // would need to employ a connection checking thread
    // We implemented this, see e.g. line 594 to 630, but at the end commented it out
    // as with our small, fast running requests it seemed not to be worth the cost.
    // You may uncomment, all tests still pass.
}

// Worker handling translation or transcription
// each worker has a tasklist and a condition variable to wake it up
struct Worker {
    tasklist: Arc<RwLock<Vec<Arc<Task>>>>,
    awake: Arc<(Mutex<bool>, Condvar)>
}

// RequestHandler handling requests from clients
// each request handler has a streamlist and a condition variable to wake it up
struct RequestHandler {
    streamlist: Arc<RwLock<Vec<TcpStream>>>,
    awake: Arc<(Mutex<bool>, Condvar)>
}

// Internal representation of a request
// each handling request has a request, a condition variable on which finishing is signaled,
// a result, a translate flag and a client on behalf of which the work is done
struct HandlingRequest {
    request: String,
    finished: Arc<(Mutex<bool>, Condvar)>,
    result: Arc<Mutex<String>>,
    translate: bool,
    client: String
}

// Server settings; allows to configure the concurrency of the server
// represents the capacities of the workers and handlers
struct Capacities {
    max_request_workers: usize,
    max_work_workers: usize,
    max_work_workers_per_request: usize,
    minimum_codon_processing_unit: usize,
    // depending on the size of the total workload different splits might make more or less sense
    // for instance, for a request of 12 codons it might not make sense to split it into 12 tasks
    // as the overhead of splitting, waiting for each task and combining is too high
    // finding the best split is quite intricate - here we just enable setting a minimum
    // workload size, from which onward we split into max_work_workers_per_request tasks
}

//////////////////// Basic Functionality & Helpers ////////////////////

// function to check if the DNA sequence is valid
// DNA sequence is valid if it only contains the letters C, G, A and T
// and its length is a multiple of three
fn is_valid_dna_sequence(input: &str) -> bool {
    let re = Regex::new(r"^[CGAT]+$").unwrap();
    re.is_match(input) && input.len() % 3 == 0
}

// function to transcribe DNA to mRNA by replacing all T's with U's
fn transcribe_dna(dna: &str) -> String {    
    let re = Regex::new(r"[T]").unwrap();
    let result = re.replace_all(dna, "U");
    result.to_string()
}

// function to get the index of the lowest value in a vector
// if there are multiple equally low values, take a random one
fn get_index_of_lowest(sizes: Vec<usize>) -> usize {
    let mut indices: Vec<usize> = (0..sizes.len()).collect();
    indices.sort_by(|&a, &b| sizes[a].cmp(&sizes[b]));
    // if there are multiple equally low sizes, take a random one
    let mut lowest_until = 0;
    for i in 1..sizes.len() {
        if sizes[i] == sizes[0] {
            lowest_until = i;
        } else {
            break;
        }
    }
    if lowest_until > 0 {
        let mut rng = rand::thread_rng();
        let index = rng.gen_range(0..=lowest_until);
        return indices[index];
    } else {
        return indices[0];
    }
}

// function to translate triples of mRNA to amino acids
fn translate_codon(codon: &str) -> String {
    match codon {
        "UUU" | "UUC"                                   => "Phe".to_string(),
        "UUA" | "UUG" | "CUU" | "CUC" | "CUA" | "CUG"   => "Leu".to_string(),
        "AUU" | "AUC" | "AUA"                           => "Ile".to_string(),
        "AUG"                                           => "Met".to_string(),
        "GUU" | "GUC" | "GUA" | "GUG"                   => "Val".to_string(),
        "UCU" | "UCC" | "UCA" | "UCG"                   => "Ser".to_string(),
        "CCU" | "CCC" | "CCA" | "CCG"                   => "Pro".to_string(),
        "ACU" | "ACC" | "ACA" | "ACG"                   => "Thr".to_string(),
        "GCU" | "GCC" | "GCA" | "GCG"                   => "Ala".to_string(),
        "UAU" | "UAC"                                   => "Tyr".to_string(),
        "UAA" | "UAG" | "UGA"                           => "STOP".to_string(),
        "CAU" | "CAC"                                   => "His".to_string(),
        "CAA" | "CAG"                                   => "Gln".to_string(),
        "AAU" | "AAC"                                   => "Asn".to_string(),
        "AAA" | "AAG"                                   => "Lys".to_string(),
        "GAU" | "GAC"                                   => "Asp".to_string(),
        "GAA" | "GAG"                                   => "Glu".to_string(),
        "UGU" | "UGC"                                   => "Cys".to_string(),
        "UGG"                                           => "Trp".to_string(),
        "CGU" | "CGC" | "CGA" | "CGG" | "AGA" | "AGG"   => "Arg".to_string(),
        "AGU" | "AGC"                                   => "Ser".to_string(),
        "GGU" | "GGC" | "GGA" | "GGG"                   => "Gly".to_string(),
        _     => "Invalid nucleotide sequence".to_string(),
    }
}

// function which splits given mRNA into codons and calls 
// the translation function (translate_codon) on each codon
fn translate(mRNA: &str) -> String {
    let mut result = String::new();
    let mut i = 0;
    while i < mRNA.len() {
        let codon = &mRNA[i..i+3];                     // get codon
        result.push_str(&translate_codon(codon));    // result of translate_codon is a pushed to the result string
        i += 3;                                             // move to next codon
    }
    return result;
}

// function takes codons and number of workers and returns
// workload per worker with division as equally as possible
fn workload_divider(workload_length: usize, num_workers: usize, in_triplets: bool) -> Vec<usize> {
    let mut workload = Vec::new();
    let mut workload_length = workload_length;

    if in_triplets {
        workload_length = workload_length / 3;
    }

    let workload_per_worker = workload_length / num_workers;
    let mut remainder = workload_length % num_workers;   // remainder

    for _ in 0..num_workers {                                   // push workload per worker to workload vector
        if remainder > 0 {                                      // equal distribution of remainder
            workload.push(workload_per_worker + 1);
            remainder -= 1;                                     
        } else {
            workload.push(workload_per_worker);
        }
    }
    if in_triplets {
        for i in 0..workload.len() {
            workload[i] = workload[i] * 3;
        }
    }
    return workload;
}

// function that can be used to log the workload of the request handlers and workers
fn workload_logger(workers: Arc<Vec<Arc<Worker>>>, handlers: Arc<Vec<Arc<RequestHandler>>>) {
    // all 20ms save the lengths of the working lists to files
    // add lengths to csv files (one file for tasklists, one for streamlists)
    let mut tasklist_file = std::fs::OpenOptions::new()
        .append(true)
        .open("logging/tasklist_lengths.csv")
        .unwrap();  
    let mut streamlist_file = std::fs::OpenOptions::new()
        .append(true)
        .open("logging/streamlist_lengths.csv")
        .unwrap();

    // start timer
    let start = Instant::now();

    loop {
        // sleep 20ms
        thread::sleep(Duration::from_millis(20));
        // get lengths of tasklists
        let mut tasklist_lengths: Vec<usize> = Vec::new();
        for worker in &*workers {
            tasklist_lengths.push(worker.tasklist.read().unwrap().len());
        }
        // get lengths of streamlists
        let mut streamlist_lengths: Vec<usize> = Vec::new();
        for handler in &*handlers {
            streamlist_lengths.push(handler.streamlist.read().unwrap().len());
        }
        
        let mut tasklist_string = String::new();
        let mut streamlist_string = String::new();

        // the time should also be logged
        tasklist_string.push_str(&format!("{},", start.elapsed().as_millis()));
        streamlist_string.push_str(&format!("{},", start.elapsed().as_millis()));

        for i in 0..tasklist_lengths.len() {
            tasklist_string.push_str(&format!("{},", tasklist_lengths[i]));
        }
        for i in 0..streamlist_lengths.len() {
            streamlist_string.push_str(&format!("{},", streamlist_lengths[i]));
        }

        tasklist_string.push_str("\n");
        streamlist_string.push_str("\n");

        // write to the logging files
        tasklist_file.write(tasklist_string.as_bytes()).unwrap();
        streamlist_file.write(streamlist_string.as_bytes()).unwrap();

    }
}

/////////////////// Worker Thread Functionality ///////////////////////

// function to start a worker thread, when there are no more tasks
// to do, the workers goes to sleep and waits for a notification
// that there is new work to do
fn run_worker(worker: Arc<Worker>) {
    println!("[Server] Worker {:?} is running", thread::current().id());
    loop {
        // wait for notification
        let &(ref lock, ref cvar) = &*worker.awake;
        let mut awake = lock.lock().unwrap();
        while !*awake {
            println!("[Server] Worker {:?} awaits task", thread::current().id());
            awake = cvar.wait(awake).unwrap();
        }
        // check if there is work to do
        let tasklist = Arc::clone(&worker.tasklist);
        let mut tasklist = tasklist.write().unwrap();
        if tasklist.len() > 0 {
            // get task from tasklist
            let task = tasklist.pop().unwrap();
            // drop tasklist lock, so new tasks can be added
            drop(tasklist);
            // do work; if the task has the valid flag
            // validity can be set to false, if the client disconnects
            if task.valid.load(std::sync::atomic::Ordering::Relaxed) {
                work(task);
            } else {
                // if the task is not valid, just set it to finished
                let mut finished = task.finished.0.lock().unwrap();
                *finished = true;
                task.finished.1.notify_one();
            }
        } else {
            // if there is no work to do, go to sleep
            *awake = false;
        }
    }
}


// function for worker to do the work on the task
// if the task is a transcription task, it calls the transcribe_dna and after that the translate fnúnction
// if the task is a translation task, it calls the translate function
fn work(task: Arc<Task>) {
    println!("[Server for Client {:?}] Worker {:?} is working on task of length {}", task.client, thread::current().id(), task.request.len());
    let mut finished = task.finished.0.lock().unwrap();
    let mut result = task.result.lock().unwrap();
    // transcription or translation
    if task.translate {
        let intermediate = transcribe_dna(&task.request);
        *result = translate(&intermediate);
    } else {
        *result = transcribe_dna(&task.request);
    }
    *finished = true;               // set finished to true
    task.finished.1.notify_one();   // notify the condition variable
}

////////////////////// Request Handler Functionality //////////////////////


// function to start a request handler thread, sleeps when nothing is to be done
// and waits for a notification that there are new requests to handle
// very similar to the worker thread, as a possible improvement the logic
// of this could be abstracted into one function
fn run_request_handler(request_handler: Arc<RequestHandler>, capacities: Arc<Capacities>, workers: Arc<Vec<Arc<Worker>>>) {
    println!("[Server] Request-handler {:?} is running", thread::current().id());
    loop {
        let cap = Arc::clone(&capacities);
        // wait for notification
        let &(ref lock, ref cvar) = &*request_handler.awake;
        let mut awake = lock.lock().unwrap();   
        while !*awake {     // wait for notify
            println!("[Server] Request Handler {:?} awaits task", thread::current().id());
            awake = cvar.wait(awake).unwrap();
        }
        println!("[Server] Request Handler {:?} received task", thread::current().id()); 
        // check if there are requests to handle
        let streamlist = Arc::clone(&request_handler.streamlist);
        let mut streamlist = streamlist.write().unwrap();
        if streamlist.len() > 0 {
            // get task from tasklist
            let stream: TcpStream = streamlist.pop().unwrap();
            // drop tasklist lock, so new tasks can be added
            drop(streamlist);
            // do work - handle stream
            handle_stream(stream, cap, Arc::clone(&workers));
        } else {
            // if there is no work to do, go to sleep
            *awake = false;
        }
    }
}


// function to handle the stream from a client; at the beginning
// it sends a message (response) to the client that the server is ready
// after that it waits for a request from the client
// and returns the result of the request to the client, as soon as the
// request is finished
fn handle_stream(mut stream: TcpStream, cap: Arc<Capacities>, workers: Arc<Vec<Arc<Worker>>>) {

    // write to the stream that the server is ready
    // using a different message type would be more readable
    let response = Response {
        result_type: "Ready".to_string(),
        data: "Server is ready".to_string(),
        valid: true,
    }; 
    let response_json = serde_json::to_string(&response).unwrap();
    stream.write(response_json.as_bytes()).unwrap();
    stream.flush().unwrap();

    loop {
        // read request from client
        // human genome ~ 3 billion base pairs
        // see https://i-base.info/ttfa/hiv-and-drug-resistance/appendix-2-supplementary-information-about-genetics/
        // with this buffer size, requests of up to ~100k codons can be handled
        let mut buffer = [0; 524288];

        // inactive clients are disconnected after 10 seconds
        // to make room for handling new clients
        stream.set_read_timeout(Some(Duration::new(10, 0))).unwrap();
        let result = stream.read(&mut buffer);
        match result {
            Ok(0) => {
                // Zero-byte read, client has closed the connection
                println!("[Server] Client disconnected.");
                break;
            }
            // linux timeout
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                println!("[Server] Client timed out.");
                break;
            }
            // windows timeout
            Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {
                println!("[Server] Client timed out.");
                break;
            }
            // other errors
            Err(_) => {
                println!("[Server] Client disconnected.");
                break;
            }
            // otherwise continue
            _ => {}
        }

        // deserialize request from the client
        let request_data = &buffer[0..result.unwrap()];
        let request: Request = match serde_json::from_slice(request_data) {
            Ok(r) => r,                         //deserialize request 
            Err(e) => {                         // if error, send error message to client
                let response = Response {
                    result_type: "Error".to_string(),
                    data: format!("Invalid JSON: {:?}", e),
                    valid: false,
                };
                // serialize error response and send to client
                let response_json = serde_json::to_string(&response).unwrap();
                stream.write(response_json.as_bytes()).unwrap();
                stream.flush().unwrap();
                break;
            }
        };
    
        // check if received DNA sequence is valid
        let is_valid = is_valid_dna_sequence(&request.data);
    
        // Handle the request based on the request_type
        let response = match request.request_type.as_str() {
            "-transcribe" => {
                // this might be used for further connection checking
                let stream_clone = stream.try_clone();
                // handle error if stream_clone fails
                let stream_for_conn_check = match stream_clone {
                    Ok(s) => s,
                    Err(e) => {
                        println!("[Server to Client {:?}] Failed, client disconnected.", stream.peer_addr().unwrap());
                        break;
                    }
                };
                // create trancription request
                let result: String = get_request_result(request.data.clone(), false, Arc::clone(&cap), Arc::clone(&workers), stream.peer_addr().unwrap().to_string(), stream_for_conn_check);

                // generate response of transcribed data for the client
                Response {
                    result_type: "Transcribed".to_string(),
                    data: format!("{}", result),
                    valid: is_valid,
                }
            }
            "-translate" => {
                // this might be used for further connection checking
                let stream_clone = stream.try_clone();
                // handle error if stream_clone fails
                let stream_for_conn_check = match stream_clone {
                    Ok(s) => s,
                    Err(e) => {
                        println!("[Server to Client {:?}] Failed, client disconnected.", stream.peer_addr().unwrap());
                        break;
                    }
                };
                // create translation request
                let result: String = get_request_result(request.data.clone(), true, Arc::clone(&cap), Arc::clone(&workers), stream.peer_addr().unwrap().to_string(), stream_for_conn_check);
                Response {
                    result_type: "Translated".to_string(),
                    data: format!("{}", result),
                    valid: is_valid && result.starts_with("Met") && result.ends_with("STOP"), 
                }
            }
            _ => {
                // otherwise send error message to client
                Response {
                    result_type: "Error".to_string(),
                    data: "Unknown request type".to_string(),
                    valid: false,
                }
            }
        };
        // serialize response and send to client
        let response_json = serde_json::to_string(&response).unwrap();
        match stream.write(response_json.as_bytes()) {
            Ok(_) => {}
            Err(e) => {
                println!("[Server to Client {:?}] Sending the result failed, client disconnected.", stream.peer_addr().unwrap());
                break;
            }
        }
        stream.flush().unwrap();
    }
}

// function that initializes a handling request, sends it out and returns the result upon finishing
fn get_request_result(request: String, translate: bool, cap: Arc<Capacities>, workers: Arc<Vec<Arc<Worker>>>, client_addr: String, stream_for_conn_check: TcpStream) -> String {
    // Create request
    let work_finished = Arc::new((Mutex::new(false), Condvar::new()));
    let work_result = Arc::new(Mutex::new(String::new()));
    let request: HandlingRequest = HandlingRequest {
        request: request,
        finished: Arc::clone(&work_finished),
        result: Arc::clone(&work_result),
        translate: translate,
        client: client_addr
    };
    let request: Arc<HandlingRequest> = Arc::new(request);
    handle_request(Arc::clone(&request), Arc::clone(&cap), Arc::clone(&workers), stream_for_conn_check);
    // Wait for task to finish
    let &(ref lock, ref cvar) = &*request.finished;
    let mut finished = lock.lock().unwrap();
    while !*finished {
        finished = cvar.wait(finished).unwrap();
    }
    // Get result
    let result = request.result.lock().unwrap();
    return result.clone();
}

// function that distributes the work of a request to the workers (for both transcription and translation)
fn handle_request(request: Arc<HandlingRequest>, capacities: Arc<Capacities>, workers: Arc<Vec<Arc<Worker>>>, mut stream_for_conn_check: TcpStream) {

    let mut task_list: Vec<Arc<Task>> = Vec::new();
    let mut num_workers_per_request = capacities.max_work_workers_per_request;

    // if the request is too small, use only one worker
    if request.request.len() <= capacities.minimum_codon_processing_unit * 3 {
        num_workers_per_request = 1;
    }

    // once the work on behalf of the request is finished, these will be set appropriately
    let mut request_finished = request.finished.0.lock().unwrap();
    let mut request_result = request.result.lock().unwrap();

    let request_string = &request.request;
    let request_length = request_string.len();

    // divide the workload; a more fine-tuned division could be an improvement
    let divided_workload = workload_divider(request_length, num_workers_per_request, request.translate);

    let mut current_pos = 0;

    // Push some tasks to the injector.
    for i in 0..num_workers_per_request {

        // generate a task
        let work_finished = Arc::new((Mutex::new(false), Condvar::new()));
        let work_result = Arc::new(Mutex::new(String::new()));
        let task = Arc::new(Task {
            request: request_string[current_pos..current_pos + divided_workload[i]].to_string(),
            finished: Arc::clone(&work_finished),
            result: Arc::clone(&work_result),
            translate: request.translate,
            client: request.client.clone(),
            valid: AtomicBool::new(true)
        });
        let workers = Arc::clone(&workers);

        // push the task to worker with shortest tasklist
        let indices: Vec<usize> = (0..workers.len()).collect();
        let list_lengths: Vec<usize> = indices.iter().map(|&i| workers[i].tasklist.read().unwrap().len()).collect();
        let worker = workers[get_index_of_lowest(list_lengths)].clone();
        worker.tasklist.write().unwrap().push(Arc::clone(&task));

        // if the worker is not awake, wake it up
        let &(ref lock, ref cvar) = &*worker.awake;
        let mut awake = lock.lock().unwrap();
        if !*awake {    // if not awake, wake up
            *awake = true;
            cvar.notify_one();
        }

        // push task to tasklist
        task_list.push(task.clone());

        // move current_pos
        current_pos += divided_workload[i];
    }

    // Initialize an empty string to store all results
    let mut results = String::new();

    // we might want to do a connection checking thread here
    // wich checks if the client is still connected
    // and otherwise marks the request as invalid
    // spawn a thread that checks if the client is still connected
    // set all tasks to invalid if the client is not connected anymore
    let task_list_arc = Arc::new(task_list);
    // let task_list_arc_clone = Arc::clone(&task_list_arc);
    // let handle = thread::spawn(move || {
    //     // set read timeout to 1ms
    //     stream_for_conn_check.set_read_timeout(Some(Duration::from_millis(10))).expect("Failed to set read timeout");
    //     loop {
    //         // sleep 10 ms
    //         thread::sleep(Duration::from_millis(10));
    //         // Try to read 1 byte from the stream
    //         let mut buffer = [0; 1];
    //         match stream_for_conn_check.peek(&mut buffer) {
    //             Ok(_) => {
    //                 // Connection is still alive
    //                 return;
    //             }
    //             // on linux
    //             Err(ref err) if err.kind() == std::io::ErrorKind::WouldBlock => {
    //                 // Timeout expired with no data read, but connection is still alive
    //                 return;
    //             }
    //             // on windows
    //             Err(ref err) if err.kind() == std::io::ErrorKind::TimedOut => {
    //                 // Timeout expired with no data read, but connection is still alive
    //                 return;
    //             }
    //             Err(_) => {
    //                 // Other errors, connection might be closed
    //                 println!("[Server] Client disconnected.");
    //                 // set all tasks to invalid
    //                 for task in task_list_arc_clone.iter() {
    //                     task.valid.store(false, std::sync::atomic::Ordering::Relaxed);
    //                 }
    //                 return;
    //             }
    //         }
    //     }
    // });

    // wait for all notifications on the condition variables
    for task in task_list_arc.iter() {
        let &(ref lock, ref cvar) = &*task.finished;
        let mut finished = lock.lock().unwrap();
        while !*finished {
            finished = cvar.wait(finished).unwrap();
        }
        // Append the result to the results string instead of printing it
        results.push_str(&format!("{}", *task.result.lock().unwrap()));
    }

    // set the request result to the results string
    *request_result = results;
    // set the request finished to true
    *request_finished = true;
    // notify the request condition variable
    request.finished.1.notify_one();
}

// function to start the server
fn start_server(capacities: Arc<Capacities>, port: String, log_workloads: bool) {
    // print boot message
    println!("[Server] 🛈: Starting Server on Machine with {} cores", num_cpus::get());

    // start listener
    //0.0.0.0:port
    let host_port = "0.0.0.0:".to_string() + &port;

    let listener = TcpListener::bind(host_port).expect("Failed to bind to address");
    println!("[Server] Listening on port {}...", port);

    // create worker list with max number of workers
    let mut workers: Vec<Arc<Worker>> = Vec::new();
    for _ in 0..capacities.max_work_workers {
        let tasklist: Arc<RwLock<Vec<Arc<Task>>>> = Arc::new(RwLock::new(Vec::new()));
        let awake: Arc<(Mutex<bool>, Condvar)> = Arc::new((Mutex::new(false), Condvar::new()));
        let worker = Worker {
            tasklist: Arc::clone(&tasklist),
            awake: Arc::clone(&awake)
        };
        // push worker to worker list
        workers.push(Arc::new(worker));
    }
    let workers = Arc::new(workers);

    // start worker threads
    for worker in &*workers {
        let worker = Arc::clone(&worker);
        let handle = thread::spawn(move || {
            run_worker(worker);
        });
    }

    // create list of request handlers
    let mut request_handlers: Vec<Arc<RequestHandler>> = Vec::new();
    for _ in 0..capacities.max_request_workers {
        let streamlist: Arc<RwLock<Vec<TcpStream>>> = Arc::new(RwLock::new(Vec::new()));
        let awake: Arc<(Mutex<bool>, Condvar)> = Arc::new((Mutex::new(false), Condvar::new()));
        let request_handler = RequestHandler {
            streamlist: Arc::clone(&streamlist),
            awake: Arc::clone(&awake)
        };
        // push request handler to request handler list
        request_handlers.push(Arc::new(request_handler));
    }
    let request_handlers = Arc::new(request_handlers);

    // start request handler threads
    for request_handler in &*request_handlers {
        let cap = Arc::clone(&capacities);
        let workers = Arc::clone(&workers);
        let request_handler = Arc::clone(&request_handler);
        let handle = thread::spawn(move || {
            run_request_handler(request_handler, cap, workers);
        });
    }

    if log_workloads {
        let workers = Arc::clone(&workers);
        let request_handlers = Arc::clone(&request_handlers);
        let handle = thread::spawn(move || {
            workload_logger(workers, request_handlers);
        });
    }

    // accept connections of incoming clients and push them to the 
    // request handler with the shortest streamlist and wake up the
    // request handler if it is not awake
    // if error, print error message that stream is not available
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => { // if stream is available
                println!("[Server] New connection: {}", stream.peer_addr().unwrap());
                // push stream to request_handler with shortest streamlist
                // and wake up the request_handler if it is not awake

                // take the request_handler with the shortest streamlist
                let indices: Vec<usize> = (0..request_handlers.len()).collect();
                let list_lengths: Vec<usize> = indices.iter().map(|&i| request_handlers[i].streamlist.read().unwrap().len()).collect();

                // push stream to request_handler with shortest streamlist
                let mut request_handler = request_handlers[get_index_of_lowest(list_lengths)].clone();
                request_handler.streamlist.write().unwrap().push(stream);
                let &(ref lock, ref cvar) = &*request_handler.awake;
                let mut awake = lock.lock().unwrap();
                if !*awake {        // if not awake, wake up
                    *awake = true;
                    cvar.notify_one();
                }
            }
            Err(e) => {     // if error, print error message
                println!("[Server] Stream not available.");
            }
        }
    }
}

/////////////////////// Main Function ///////////////////////////

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut port;

    if args.len() != 2 {
        // print usage message if not valid
        println!("Usage: ./client -transcribe host:port");
        println!("Usage: ./client -translate host:port");
        std::process::exit(1);
    }

    port = args[1].clone();

    // init capacities
    let capacities = Arc::new(Capacities {
        max_request_workers: 2,
        max_work_workers: 4,
        max_work_workers_per_request: 2,
        minimum_codon_processing_unit: 3,
    });
    // start server with given capacities
    start_server(capacities, port, false);
}