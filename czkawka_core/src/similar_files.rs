use crate::common_directory::Directories;
use crate::common_messages::Messages;
use crate::common_traits::{DebugPrint, PrintResults, SaveResults};
use crossbeam_channel::Receiver;
use bk_tree::{BKTree, metrics};
use std::collections::{HashMap, BTreeMap};
use crate::common_items::ExcludedItems;
use crate::common::Common;
use std::time::SystemTime;


/// Struct to store most basics info about all folder
pub struct SimilarImages {
    information: Info,
    text_messages: Messages,
    directories: Directories,
    excluded_items: ExcludedItems,
    bktree: BKTree<String>,
    recursive_search: bool,
    image_hashes: HashMap<u64,Vec<String>>, // Hashmap with image hashes and Vector with names of files
    stopped_search: bool,
}

/// Info struck with helpful information's about results
#[derive(Default)]
pub struct Info {
}
impl Info {
    pub fn new() -> Self {
        Default::default()
    }
}

/// Method implementation for EmptyFolder
impl SimilarImages {
    /// New function providing basics values
    pub fn new() -> Self {
        Self {
            information: Default::default(),
            text_messages: Messages::new(),
            directories: Directories::new(),
            excluded_items: Default::default(),
            bktree: BKTree::new(metrics::Levenshtein),
            recursive_search: true,
            image_hashes: Default::default(),
            stopped_search: false,
        }
    }

    pub fn get_stopped_search(&self) -> bool {
        self.stopped_search
    }

    pub const fn get_text_messages(&self) -> &Messages {
        &self.text_messages
    }
    
    pub const fn get_information(&self) -> &Info {
        &self.information
    }

    pub fn set_recursive_search(&mut self, recursive_search: bool) {
        self.recursive_search = recursive_search;
    }

    /// Public function used by CLI to search for empty folders
    pub fn find_similar_images(&mut self, rx: Option<&Receiver<()>>) {
        self.directories.optimize_directories(true, &mut self.text_messages);
        if !self.check_for_similar_images(rx) {
            self.stopped_search = true;
            return;
        }
        // if self.delete_folders {
        //     self.delete_empty_folders();
        // }
        self.debug_print();
    }

    // pub fn set_delete_folder(&mut self, delete_folder: bool) {
    //     self.delete_folders = delete_folder;
    // }

    /// Function to check if folder are empty.
    /// Parameter initial_checking for second check before deleting to be sure that checked folder is still empty
    fn check_for_similar_images(&mut self, _rx: Option<&Receiver<()>>) -> bool {
        let start_time: SystemTime = SystemTime::now();
        let mut folders_to_check: Vec<String> = Vec::with_capacity(1024 * 2); // This should be small enough too not see to big difference and big enough to store most of paths without needing to resize vector

        // Add root folders for finding
        for id in &self.directories.included_directories {
            folders_to_check.push(id.to_string());
        }
        self.information.number_of_checked_folders += folders_to_check.len();

        let mut current_folder: String;
        let mut next_folder: String;
        while !folders_to_check.is_empty() {
            if rx.is_some() && rx.unwrap().try_recv().is_ok() {
                return false;
            }
            current_folder = folders_to_check.pop().unwrap();

            // Read current dir, if permission are denied just go to next
            let read_dir = match fs::read_dir(&current_folder) {
                Ok(t) => t,
                Err(_) => {
                    self.text_messages.warnings.push("Cannot open dir ".to_string() + current_folder.as_str());
                    continue;
                } // Permissions denied
            };

            // Check every sub folder/file/link etc.
            'dir: for entry in read_dir {
                let entry_data = match entry {
                    Ok(t) => t,
                    Err(_) => {
                        self.text_messages.warnings.push("Cannot read entry in dir ".to_string() + current_folder.as_str());
                        continue;
                    } //Permissions denied
                };
                let metadata: Metadata = match entry_data.metadata() {
                    Ok(t) => t,
                    Err(_) => {
                        self.text_messages.warnings.push("Cannot read metadata in dir ".to_string() + current_folder.as_str());
                        continue;
                    } //Permissions denied
                };
                if metadata.is_dir() {
                    self.information.number_of_checked_folders += 1;

                    if !self.recursive_search {
                        continue;
                    }

                    next_folder = "".to_owned()
                        + &current_folder
                        + match &entry_data.file_name().into_string() {
                        Ok(t) => t,
                        Err(_) => continue,
                    }
                        + "/";

                    for ed in &self.directories.excluded_directories {
                        if next_folder == *ed {
                            continue 'dir;
                        }
                    }
                    for expression in &self.excluded_items.items {
                        if Common::regex_check(expression, &next_folder) {
                            continue 'dir;
                        }
                    }
                    folders_to_check.push(next_folder);
                } else if metadata.is_file() {
                    // let mut have_valid_extension: bool;
                    let file_name_lowercase: String = match entry_data.file_name().into_string() {
                        Ok(t) => t,
                        Err(_) => continue,
                    }
                        .to_lowercase();

                    // Checking allowed extensions
                    if !self.allowed_extensions.file_extensions.is_empty() {
                        let allowed = self.allowed_extensions.file_extensions.iter().any(|e| file_name_lowercase.ends_with((".".to_string() + e.to_lowercase().as_str()).as_str()));
                        if !allowed {
                            // Not an allowed extension, ignore it.
                            self.information.number_of_ignored_files += 1;
                            continue 'dir;
                        }
                    }
                    // Checking files
                    if metadata.len() >= self.minimal_file_size {
                        #[allow(unused_mut)] // Used is later by Windows build
                            let mut current_file_name = "".to_owned()
                            + &current_folder
                            + match &entry_data.file_name().into_string() {
                            Ok(t) => t,
                            Err(_) => continue,
                        };

                        // Checking expressions
                        for expression in &self.excluded_items.items {
                            if Common::regex_check(expression, &current_file_name) {
                                continue 'dir;
                            }
                        }

                        #[cfg(target_family = "windows")]
                            {
                                current_file_name = Common::prettier_windows_path(&current_file_name);
                            }

                        // Creating new file entry
                        let fe: FileEntry = FileEntry {
                            path: current_file_name.clone(),
                            size: metadata.len(),
                            modified_date: match metadata.modified() {
                                Ok(t) => match t.duration_since(UNIX_EPOCH) {
                                    Ok(d) => d.as_secs(),
                                    Err(_) => {
                                        self.text_messages.warnings.push(format!("File {} seems to be modified before Unix Epoch.", current_file_name));
                                        0
                                    }
                                },
                                Err(_) => {
                                    self.text_messages.warnings.push("Unable to get modification date from file ".to_string() + current_file_name.as_str());
                                    continue;
                                } // Permissions Denied
                            },
                        };

                        // Adding files to BTreeMap
                        self.files_with_identical_size.entry(metadata.len()).or_insert_with(Vec::new);
                        self.files_with_identical_size.get_mut(&metadata.len()).unwrap().push(fe);

                        self.information.number_of_checked_files += 1;
                    } else {
                        // Probably this is symbolic links so we are free to ignore this
                        self.information.number_of_ignored_files += 1;
                    }
                } else {
                    // Probably this is symbolic links so we are free to ignore this
                    self.information.number_of_ignored_things += 1;
                }
            }
        }

        // Create new BTreeMap without single size entries(files have not duplicates)
        let mut new_map: BTreeMap<u64, Vec<FileEntry>> = Default::default();

        self.information.number_of_duplicated_files_by_size = 0;

        for (size, vector) in &self.files_with_identical_size {
            if vector.len() > 1 {
                self.information.number_of_duplicated_files_by_size += vector.len() - 1;
                self.information.number_of_groups_by_size += 1;
                self.information.lost_space_by_size += (vector.len() as u64 - 1) * size;
                new_map.insert(*size, vector.clone());
            }
        }
        self.files_with_identical_size = new_map;

        Common::print_time(start_time, SystemTime::now(), "check_for_similar_images".to_string());






        true
    }

    /// Set included dir which needs to be relative, exists etc.
    pub fn set_included_directory(&mut self, included_directory: String) {
        self.directories.set_included_directory(included_directory, &mut self.text_messages);
    }
    
    pub fn set_excluded_directory(&mut self, excluded_directory: String) {
        self.directories.set_excluded_directory(excluded_directory, &mut self.text_messages);
    }

    pub fn set_excluded_items(&mut self, excluded_items: String) {
        self.excluded_items.set_excluded_items(excluded_items, &mut self.text_messages);
    }




}
impl Default for SimilarImages {
    fn default() -> Self {
        Self::new()
    }
}

impl DebugPrint for SimilarImages {
    #[allow(dead_code)]
    #[allow(unreachable_code)]
    fn debug_print(&self) {
        #[cfg(not(debug_assertions))]
            {
                return;
            }

        // println!("---------------DEBUG PRINT---------------");
        // println!("Number of all checked folders - {}", self.information.number_of_checked_folders);
        // println!("Number of empty folders - {}", self.information.number_of_empty_folders);
        // println!("Included directories - {:?}", self.directories.included_directories);
        // println!("-----------------------------------------");
    }
}
impl SaveResults for SimilarImages {
    fn save_results_to_file(&mut self, _file_name: &str) -> bool {
        // let start_time: SystemTime = SystemTime::now();
        // let file_name: String = match file_name {
        //     "" => "results.txt".to_string(),
        //     k => k.to_string(),
        // };
        //
        // let mut file = match File::create(&file_name) {
        //     Ok(t) => t,
        //     Err(_) => {
        //         self.text_messages.errors.push("Failed to create file ".to_string() + file_name.as_str());
        //         return false;
        //     }
        // };
        //
        // match file.write_all(format!("Results of searching {:?} with excluded directories {:?}\n", self.directories.included_directories, self.directories.excluded_directories).as_bytes()) {
        //     Ok(_) => (),
        //     Err(_) => {
        //         self.text_messages.errors.push("Failed to save results to file ".to_string() + file_name.as_str());
        //         return false;
        //     }
        // }
        //
        // if !self.empty_folder_list.is_empty() {
        //     file.write_all(b"-------------------------------------------------Empty folder list-------------------------------------------------\n").unwrap();
        //     file.write_all(("Found ".to_string() + self.information.number_of_empty_folders.to_string().as_str() + " empty folders\n").as_bytes()).unwrap();
        //     for name in self.empty_folder_list.keys() {
        //         file.write_all((name.clone() + "\n").as_bytes()).unwrap();
        //     }
        // } else {
        //     file.write_all(b"Not found any empty folders.").unwrap();
        // }
        // Common::print_time(start_time, SystemTime::now(), "save_results_to_file".to_string());
        // true
        true
    }
}
impl PrintResults for SimilarImages {
    /// Prints basic info about empty folders // TODO print better
    fn print_results(&self) {
        // if !self.empty_folder_list.is_empty() {
        //     println!("Found {} empty folders", self.empty_folder_list.len());
        // }
        // for name in self.empty_folder_list.keys() {
        //     println!("{}", name);
        // }
    }
}
