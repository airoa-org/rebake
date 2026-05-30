use std::collections::HashMap;
use std::fs;
use std::io::{self, Write};

use camino::Utf8Path;
use serde::{Deserialize, Serialize};

use crate::core::stage::StageError;

use super::Info;

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct LeRobotMetadata {
    pub info: Info,
    pub tasks: LeRobotTasks,
}

impl LeRobotMetadata {
    pub fn get_task_index(&self, task: &str) -> Option<&usize> {
        self.tasks.task_to_task_index.get(task)
    }

    pub fn add_task(&mut self, task: String) {
        self.tasks
            .task_to_task_index
            .insert(task, self.info.total_tasks);
        self.info.total_tasks += 1;
    }
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct LeRobotInfo {
    pub total_tasks: usize,
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct LeRobotTasks {
    pub task_to_task_index: HashMap<String, usize>,
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct LeRobotTasksVec {
    pub tasks: Vec<LeRobotTask>,
}

impl LeRobotTasksVec {
    pub fn save(&mut self, outdir: &Utf8Path) -> Result<(), StageError> {
        let path = outdir.join("meta/tasks.jsonl");
        if let Some(parent) = path.parent()
            && !parent.exists()
        {
            fs::create_dir_all(parent)?;
        }
        let mut writer = io::BufWriter::new(fs::File::create(path)?);
        self.tasks.sort_by(|a, b| a.task_index.cmp(&b.task_index));
        for task in self.tasks.iter() {
            serde_json::to_writer(&mut writer, task)?;
            writeln!(&mut writer)?;
        }
        writer.flush()?;
        Ok(())
    }
}

impl From<LeRobotTasks> for LeRobotTasksVec {
    fn from(tasks: LeRobotTasks) -> Self {
        let mut tasks = tasks
            .task_to_task_index
            .iter()
            .map(|(task, index)| LeRobotTask {
                task_index: *index,
                task: task.clone(),
            })
            .collect::<Vec<_>>();
        tasks.sort_by(|a, b| a.task_index.cmp(&b.task_index));
        Self { tasks }
    }
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct LeRobotTask {
    pub task_index: usize,
    pub task: String,
}
