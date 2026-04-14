// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Run command output result type.

pub(crate) struct RunResult {
    pub status: String,
    pub message: String,
    pub job_name: String,
    pub step_name: Option<String>,
    pub duration: f64,
    pub chunks_run: usize,
}

impl crate::common::CliResult for RunResult {
    fn to_json_value(&self) -> serde_json::Value {
        serde_json::json!({
            "status": self.status,
            "message": self.message,
            "job_name": self.job_name,
            "step_name": self.step_name,
            "duration": self.duration,
            "chunks_run": self.chunks_run,
        })
    }
}

impl std::fmt::Display for RunResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f)?;
        writeln!(f, "--- Results of local session ---")?;
        writeln!(f)?;
        writeln!(f, "{}", self.message)?;
        writeln!(f)?;
        writeln!(f, "Job: {}", self.job_name)?;
        if let Some(sn) = &self.step_name {
            writeln!(f, "Step: {sn}")?;
        }
        writeln!(f, "Duration: {:.3} seconds", self.duration)?;
        write!(f, "Chunks run: {}", self.chunks_run)
    }
}
