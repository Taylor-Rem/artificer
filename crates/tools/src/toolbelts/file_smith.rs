use anyhow::Result;
use serde_json::json;
use std::fs;
use std::path::PathBuf;

use crate::ToolLocation;
use crate::register_toolbelt;


pub struct FileSmith {
    directory: PathBuf,
}

impl Default for FileSmith {
    fn default() -> Self {
        Self {
            directory: std::env::current_dir().unwrap_or_else(|_| PathBuf::from("../../../../../../../..")),
        }
    }
}

register_toolbelt! {
    FileSmith {
        description: "Tool for interacting with files and directories and performing related task",
        location: ToolLocation::Client,
        tools: {
            "read_file" => read_file {
                description: "Reads the entire contents of a file and returns it as a string",
                params: ["path": "string" => "Path to the file to read"]
            },
            "write_file" => write_file {
                description: "Writes content to a file, overwriting existing content",
                params: [
                    "path": "string" => "Path to the file to write",
                    "content": "string" => "Content to write to the file"
                ]
            },
            "replace_text" => replace_text {
                description: "Replaces the first occurrence of old_text with new_text in a file. Case-sensitive. Returns error if old_text not found.",
                params: [
                    "path": "string" => "Path to the file to modify",
                    "old_text": "string" => "Text to find (exact match)",
                    "new_text": "string" => "Replacement text"
                ]
            },
            "insert_at_line" => insert_at_line {
                description: "Inserts content at the specified line number (1-indexed). If line number exceeds file length, inserts at end.",
                params: [
                    "path": "string" => "Path to the file to modify",
                    "line_number": "integer" => "Line number to insert at (1-indexed, 0 inserts at beginning)",
                    "content": "string" => "Content to insert"
                ]
            },
            "append_file" => append_file {
                description: "Appends content to the end of a file. Creates the file if it doesn't exist.",
                params: [
                    "path": "string" => "Path to the file to append to",
                    "content": "string" => "Content to append"
                ]
            },
            "copy_file" => copy_file {
                description: "Copies a file from source to destination",
                params: [
                    "source": "string" => "Path to the source file",
                    "destination": "string" => "Path to the destination file"
                ]
            },
            "move_file" => move_file {
                description: "Moves a file from source to destination",
                params: [
                    "source": "string" => "Path to the source file",
                    "destination": "string" => "Path to the destination"
                ]
            },
            "rename_file" => rename_file {
                description: "Renames a file",
                params: [
                    "old_name": "string" => "Current file name/path",
                    "new_name": "string" => "New file name/path"
                ]
            },
            "delete_file" => delete_file {
                description: "Deletes a file",
                params: ["path": "string" => "Path to the file to delete"]
            },
            "file_exists" => file_exists {
                description: "Checks if a file or directory exists. Returns JSON with exists, is_file, and is_directory flags.",
                params: ["path": "string" => "Path to check"]
            },
            "get_file_info" => get_file_info {
                description: "Gets metadata about a file including size, type, permissions, and modification time",
                params: ["path": "string" => "Path to the file"]
            },
            "list_directory" => list_directory {
                description: "Lists all files and directories in the specified directory. Returns JSON array of names.",
                params: ["path": "string" => "Path to the directory to list (defaults to current directory)"]
            },
            "create_directory" => create_directory {
                description: "Creates a directory and all parent directories if they don't exist",
                params: ["path": "string" => "Path to the directory to create"]
            },
            "delete_directory" => delete_directory {
                description: "Deletes a directory. Use recursive=true to delete non-empty directories.",
                params: [
                    "path": "string" => "Path to the directory to delete",
                    "recursive": "boolean" => "Whether to delete directory contents recursively (default: false)"
                ]
            },
            "search_files" => search_files {
                description: "Recursively searches for files matching a pattern. Returns JSON with matches and count.",
                params: [
                    "pattern": "string" => "Pattern to search for in filenames",
                    "path": "string" => "Directory to search in (defaults to current directory)"
                ]
            }
        }
    }
}

impl FileSmith {
    fn read_file(&self, args: &serde_json::Value) -> Result<String> {
        let path = args["path"].as_str().unwrap_or("");
        let full_path = self.directory.join(path);
        match fs::read_to_string(&full_path) {
            Ok(content) => Ok(content),
            Err(e) => Ok(format!("Error reading file: {}", e)),
        }
    }
    fn write_file(&self, args: &serde_json::Value) -> Result<String> {
        let path = args["path"].as_str().unwrap_or("");
        let content = args["content"].as_str().unwrap_or("");
        let full_path = self.directory.join(path);
        match fs::write(&full_path, content) {
            Ok(_) => Ok(format!("Successfully wrote to {}", path)),
            Err(e) => Ok(format!("Error writing file: {}", e)),
        }
    }
    fn list_directory(&self, args: &serde_json::Value) -> Result<String> {
        let path = args["path"].as_str().unwrap_or(".");
        let full_path = self.directory.join(path);
        match fs::read_dir(&full_path) {
            Ok(entries) => {
                let files: Vec<String> = entries
                    .filter_map(|e| e.ok())
                    .map(|e| e.file_name().to_string_lossy().to_string())
                    .collect();
                Ok(json!(files).to_string())
            }
            Err(e) => Ok(format!("Error listing directory: {}", e)),
        }
    }
    fn delete_file(&self, args: &serde_json::Value) -> Result<String> {
        let path = args["path"].as_str().unwrap_or("");
        let full_path = self.directory.join(path);
        match fs::remove_file(&full_path) {
            Ok(_) => Ok(format!("Successfully deleted {}", path)),
            Err(e) => Ok(format!("Error deleting file: {}", e)),
        }
    }
    fn create_directory(&self, args: &serde_json::Value) -> Result<String> {
        let path = args["path"].as_str().unwrap_or("");
        let full_path = self.directory.join(path);
        match fs::create_dir_all(&full_path) {
            Ok(_) => Ok(format!("Successfully created directory {}", path)),
            Err(e) => Ok(format!("Error creating directory: {}", e)),
        }
    }
    fn append_file(&self, args: &serde_json::Value) -> Result<String> {
        use std::fs::OpenOptions;
        use std::io::Write;

        let path = args["path"].as_str().unwrap_or("");
        let content = args["content"].as_str().unwrap_or("");
        let full_path = self.directory.join(path);

        match OpenOptions::new().append(true).create(true).open(&full_path) {
            Ok(mut file) => {
                match file.write_all(content.as_bytes()) {
                    Ok(_) => Ok(format!("Successfully appended to {}", path)),
                    Err(e) => Ok(format!("Error writing to file: {}", e)),
                }
            }
            Err(e) => Ok(format!("Error opening file: {}", e)),
        }
    }
    fn insert_at_line(&self, args: &serde_json::Value) -> Result<String> {
        let path = args["path"].as_str().unwrap_or("");
        let line_number = args["line_number"].as_u64().unwrap_or(1) as usize;
        let content = args["content"].as_str().unwrap_or("");
        let full_path = self.directory.join(path);

        match fs::read_to_string(&full_path) {
            Ok(file_content) => {
                let mut lines: Vec<&str> = file_content.lines().collect();
                let insert_idx = if line_number == 0 { 0 } else { (line_number - 1).min(lines.len()) };
                lines.insert(insert_idx, content);

                let new_content = lines.join("\n");
                match fs::write(&full_path, new_content) {
                    Ok(_) => Ok(format!("Successfully inserted at line {} in {}", line_number, path)),
                    Err(e) => Ok(format!("Error writing file: {}", e)),
                }
            }
            Err(e) => Ok(format!("Error reading file: {}", e)),
        }
    }
    fn replace_text(&self, args: &serde_json::Value) -> Result<String> {
        let path = args["path"].as_str().unwrap_or("");
        let old_text = args["old_text"].as_str().unwrap_or("");
        let new_text = args["new_text"].as_str().unwrap_or("");
        let full_path = self.directory.join(path);

        match fs::read_to_string(&full_path) {
            Ok(file_content) => {
                if !file_content.contains(old_text) {
                    return Ok(format!("Error: old_text not found in {}", path));
                }

                let new_content = file_content.replacen(old_text, new_text, 1);
                match fs::write(&full_path, new_content) {
                    Ok(_) => Ok(format!("Successfully replaced text in {}", path)),
                    Err(e) => Ok(format!("Error writing file: {}", e)),
                }
            }
            Err(e) => Ok(format!("Error reading file: {}", e)),
        }
    }
    fn copy_file(&self, args: &serde_json::Value) -> Result<String> {
        let source = args["source"].as_str().unwrap_or("");
        let destination = args["destination"].as_str().unwrap_or("");
        let source_path = self.directory.join(source);
        let dest_path = self.directory.join(destination);

        match fs::copy(&source_path, &dest_path) {
            Ok(bytes) => Ok(format!("Successfully copied {} bytes from {} to {}", bytes, source, destination)),
            Err(e) => Ok(format!("Error copying file: {}", e)),
        }
    }
    fn move_file(&self, args: &serde_json::Value) -> Result<String> {
        let source = args["source"].as_str().unwrap_or("");
        let destination = args["destination"].as_str().unwrap_or("");
        let source_path = self.directory.join(source);
        let dest_path = self.directory.join(destination);

        match fs::rename(&source_path, &dest_path) {
            Ok(_) => Ok(format!("Successfully moved {} to {}", source, destination)),
            Err(e) => Ok(format!("Error moving file: {}", e)),
        }
    }
    fn rename_file(&self, args: &serde_json::Value) -> Result<String> {
        let old_name = args["old_name"].as_str().unwrap_or("");
        let new_name = args["new_name"].as_str().unwrap_or("");
        let old_path = self.directory.join(old_name);
        let new_path = self.directory.join(new_name);

        match fs::rename(&old_path, &new_path) {
            Ok(_) => Ok(format!("Successfully renamed {} to {}", old_name, new_name)),
            Err(e) => Ok(format!("Error renaming file: {}", e)),
        }
    }
    fn file_exists(&self, args: &serde_json::Value) -> Result<String> {
        let path = args["path"].as_str().unwrap_or("");
        let full_path = self.directory.join(path);

        Ok(json!({
            "exists": full_path.exists(),
            "is_file": full_path.is_file(),
            "is_directory": full_path.is_dir()
        }).to_string())
    }
    fn get_file_info(&self, args: &serde_json::Value) -> Result<String> {
        let path = args["path"].as_str().unwrap_or("");
        let full_path = self.directory.join(path);

        match fs::metadata(&full_path) {
            Ok(metadata) => {
                let modified = metadata.modified()
                    .ok()
                    .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|duration| duration.as_secs())
                    .unwrap_or(0);

                Ok(json!({
                    "size": metadata.len(),
                    "is_file": metadata.is_file(),
                    "is_directory": metadata.is_dir(),
                    "is_symlink": metadata.is_symlink(),
                    "readonly": metadata.permissions().readonly(),
                    "modified_timestamp": modified,
                }).to_string())
            }
            Err(e) => Ok(format!("Error getting file info: {}", e)),
        }
    }
    fn delete_directory(&self, args: &serde_json::Value) -> Result<String> {
        let path = args["path"].as_str().unwrap_or("");
        let full_path = self.directory.join(path);

        // Check if recursive flag is set (default to false for safety)
        let recursive = args["recursive"].as_bool().unwrap_or(false);

        if recursive {
            match fs::remove_dir_all(&full_path) {
                Ok(_) => Ok(format!("Successfully deleted directory {} and all contents", path)),
                Err(e) => Ok(format!("Error deleting directory: {}", e)),
            }
        } else {
            match fs::remove_dir(&full_path) {
                Ok(_) => Ok(format!("Successfully deleted empty directory {}", path)),
                Err(e) => Ok(format!("Error deleting directory (must be empty or use recursive=true): {}", e)),
            }
        }
    }
    fn search_files(&self, args: &serde_json::Value) -> Result<String> {
        let pattern = args["pattern"].as_str().unwrap_or("");
        let search_path = args["path"].as_str().unwrap_or(".");
        let full_path = self.directory.join(search_path);

        fn search_recursive(dir: &std::path::Path, pattern: &str, results: &mut Vec<String>) -> std::io::Result<()> {
            if dir.is_dir() {
                for entry in fs::read_dir(dir)? {
                    let entry = entry?;
                    let path = entry.path();

                    if let Some(filename) = path.file_name() {
                        let filename_str = filename.to_string_lossy();
                        if filename_str.contains(pattern) {
                            if let Ok(relative) = path.strip_prefix(dir) {
                                results.push(relative.to_string_lossy().to_string());
                            }
                        }
                    }

                    if path.is_dir() {
                        search_recursive(&path, pattern, results)?;
                    }
                }
            }
            Ok(())
        }

        let mut results = Vec::new();
        match search_recursive(&full_path, pattern, &mut results) {
            Ok(_) => Ok(json!({
                "matches": results,
                "count": results.len()
            }).to_string()),
            Err(e) => Ok(format!("Error searching files: {}", e)),
        }
    }
}
