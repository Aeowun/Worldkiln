use std::collections::VecDeque;

/// Stable identifier for a running AeoScript instance.
///
/// This is deliberately separate from an engine Entity ID. A single engine
/// entity may eventually own multiple script components, so script execution
/// needs its own identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ScriptTaskId(pub u64);

impl ScriptTaskId {
    pub const fn new(id: u64) -> Self {
        Self(id)
    }

    pub const fn value(self) -> u64 {
        self.0
    }
}

/// Reason why a script yields control back to the engine.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum YieldReason {
    /// Resume the script once engine time reaches the requested wake time.
    WaitSeconds(f64),
}

impl YieldReason {
    pub fn validate(self) -> Result<Self, String> {
        match self {
            Self::WaitSeconds(seconds) => {
                if !seconds.is_finite() {
                    return Err("AeoScript wait duration must be finite.".to_string());
                }

                if seconds <= 0.0 {
                    return Err("AeoScript wait duration must be greater than zero.".to_string());
                }

                Ok(self)
            }
        }
    }
}

/// Current scheduler state of one script task.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ScriptTaskState {
    Ready,
    Running,
    Waiting { wake_at: f64 },
    // Worldkiln 0.8.7 | WO 0.8.7-17 | 2026-10-06 00:51 -05:00
    Paused,
    Completed,
    Failed,
}

/// Result returned by an executing script fiber.
///
/// The interpreter/VM will produce these results later. The scheduler only
/// decides when a task is eligible to run again.
#[derive(Clone, Debug, PartialEq)]
pub enum FiberResult {
    Continue,
    Yield(YieldReason),
    // Worldkiln 0.8.7 | WO 0.8.7-17 | 2026-10-06 00:51 -05:00
    Paused,
    Complete,
    Failed(String),
}

#[derive(Clone, Debug)]
struct ScriptTask {
    id: ScriptTaskId,
    state: ScriptTaskState,
}

impl ScriptTask {
    fn new(id: ScriptTaskId) -> Self {
        Self {
            id,
            state: ScriptTaskState::Ready,
        }
    }
}

/// Cooperative scheduler for AeoScript execution.
///
/// This scheduler never sleeps the host thread. Waiting scripts simply leave
/// the ready queue until their wake time is reached.
#[derive(Clone, Debug, Default)]
pub struct ScriptScheduler {
    next_id: u64,
    current_time: f64,
    tasks: Vec<ScriptTask>,
    ready_queue: VecDeque<ScriptTaskId>,
}

impl ScriptScheduler {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn current_time(&self) -> f64 {
        self.current_time
    }

    pub fn task_count(&self) -> usize {
        self.tasks.len()
    }

    pub fn ready_count(&self) -> usize {
        self.ready_queue.len()
    }

    pub fn spawn(&mut self) -> ScriptTaskId {
        let id = ScriptTaskId::new(self.next_id);

        self.next_id = self
            .next_id
            .checked_add(1)
            .expect("AeoScript task ID overflow");

        self.tasks.push(ScriptTask::new(id));

        self.ready_queue.push_back(id);

        id
    }

    pub fn spawn_waiting(&mut self, reason: YieldReason) -> Result<ScriptTaskId, String> {
        let reason = reason.validate()?;

        let wake_at = match reason {
            YieldReason::WaitSeconds(seconds) => self.current_time + seconds,
        };

        let id = ScriptTaskId::new(self.next_id);

        self.next_id = self
            .next_id
            .checked_add(1)
            .expect("AeoScript task ID overflow");

        self.tasks.push(ScriptTask {
            id,
            state: ScriptTaskState::Waiting { wake_at },
        });

        Ok(id)
    }

    pub fn state(&self, id: ScriptTaskId) -> Option<ScriptTaskState> {
        self.tasks
            .iter()
            .find(|task| task.id == id)
            .map(|task| task.state)
    }

    pub fn begin_running(&mut self, id: ScriptTaskId) -> Result<(), String> {
        let task = self.find_task_mut(id)?;

        match task.state {
            ScriptTaskState::Ready => {
                task.state = ScriptTaskState::Running;
                Ok(())
            }

            ScriptTaskState::Running => Err("script task is already running.".to_string()),

            ScriptTaskState::Waiting { .. } => Err("cannot run a waiting script task.".to_string()),

            // Worldkiln 0.8.7 | WO 0.8.7-17 | 2026-10-06 02:35 -05:00
            ScriptTaskState::Paused => Err("cannot run a paused script task.".to_string()),

            ScriptTaskState::Completed => Err("cannot run a completed script task.".to_string()),

            ScriptTaskState::Failed => Err("cannot run a failed script task.".to_string()),
        }
    }

    pub fn apply_result(&mut self, id: ScriptTaskId, result: FiberResult) -> Result<(), String> {
        match result {
            FiberResult::Continue => {
                let task = self.find_task_mut(id)?;

                if task.state != ScriptTaskState::Running {
                    return Err("script task must be running before it can continue.".to_string());
                }

                task.state = ScriptTaskState::Ready;

                self.ready_queue.push_back(id);

                Ok(())
            }

            FiberResult::Yield(reason) => {
                let reason = reason.validate()?;

                match reason {
                    YieldReason::WaitSeconds(seconds) => {
                        let wake_at = self.current_time + seconds;

                        let task = self.find_task_mut(id)?;

                        if task.state != ScriptTaskState::Running {
                            return Err(
                                "script task must be running before it can yield.".to_string()
                            );
                        }

                        task.state = ScriptTaskState::Waiting { wake_at };

                        Ok(())
                    }
                }
            }

            // Worldkiln 0.8.7 | WO 0.8.7-17 | 2026-10-06 00:51 -05:00
            FiberResult::Paused => {
                let task = self.find_task_mut(id)?;
                if task.state != ScriptTaskState::Running {
                    return Err("script task must be running before it can pause.".to_string());
                }
                task.state = ScriptTaskState::Paused;
                Ok(())
            }

            FiberResult::Complete => {
                let task = self.find_task_mut(id)?;

                if task.state != ScriptTaskState::Running {
                    return Err("script task must be running before it can complete.".to_string());
                }

                task.state = ScriptTaskState::Completed;

                Ok(())
            }

            FiberResult::Failed(message) => {
                let task = self.find_task_mut(id)?;

                if task.state != ScriptTaskState::Running {
                    return Err("script task must be running before it can fail.".to_string());
                }

                task.state = ScriptTaskState::Failed;

                let _ = message;

                Ok(())
            }
        }
    }

    /// Advances engine time and moves any expired waits back to the ready queue.
    ///
    /// Time is expected to be monotonic. Rewinding time is rejected because it
    /// would make wait scheduling ambiguous.
    pub fn tick(&mut self, new_time: f64) -> Result<(), String> {
        if !new_time.is_finite() {
            return Err("AeoScript scheduler time must be finite.".to_string());
        }

        if new_time < self.current_time {
            return Err(format!(
                "AeoScript scheduler time cannot move backwards ({} -> {}).",
                self.current_time, new_time
            ));
        }

        self.current_time = new_time;

        let mut woke = Vec::new();

        for task in &mut self.tasks {
            let wake = match task.state {
                ScriptTaskState::Waiting { wake_at } if wake_at <= self.current_time => true,

                _ => false,
            };

            if wake {
                task.state = ScriptTaskState::Ready;

                woke.push(task.id);
            }
        }

        for id in woke {
            self.ready_queue.push_back(id);
        }

        Ok(())
    }

    pub fn pop_ready(&mut self) -> Option<ScriptTaskId> {
        self.ready_queue.pop_front()
    }

    // Worldkiln 0.8.7 | WO 0.8.7-17 | 2026-10-06 00:51 -05:00
    pub fn resume_paused(&mut self, id: ScriptTaskId) -> Result<(), String> {
        let task = self.find_task_mut(id)?;
        if task.state != ScriptTaskState::Paused {
            return Err("script task is not paused.".to_string());
        }
        task.state = ScriptTaskState::Ready;
        if !self.ready_queue.contains(&id) {
            self.ready_queue.push_back(id);
        }
        Ok(())
    }

    pub fn cancel(&mut self, id: ScriptTaskId) -> Result<(), String> {
        let task = self.find_task_mut(id)?;

        match task.state {
            ScriptTaskState::Completed | ScriptTaskState::Failed => {
                Err("cannot cancel a finished script task.".to_string())
            }

            _ => {
                task.state = ScriptTaskState::Failed;
                self.ready_queue.retain(|&queued_id| queued_id != id);

                Ok(())
            }
        }
    }

    pub fn remove(&mut self, id: ScriptTaskId) -> Result<(), String> {
        let index = self
            .tasks
            .iter()
            .position(|task| task.id == id)
            .ok_or_else(|| format!("unknown AeoScript task {}", id.value()))?;

        self.tasks.remove(index);
        self.ready_queue.retain(|&queued_id| queued_id != id);

        Ok(())
    }

    fn find_task(&self, id: ScriptTaskId) -> Result<&ScriptTask, String> {
        self.tasks
            .iter()
            .find(|task| task.id == id)
            .ok_or_else(|| format!("unknown AeoScript task {}", id.value()))
    }

    fn find_task_mut(&mut self, id: ScriptTaskId) -> Result<&mut ScriptTask, String> {
        self.tasks
            .iter_mut()
            .find(|task| task.id == id)
            .ok_or_else(|| format!("unknown AeoScript task {}", id.value()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spawned_task_is_ready() {
        let mut scheduler = ScriptScheduler::new();

        let id = scheduler.spawn();

        assert_eq!(scheduler.task_count(), 1);

        assert_eq!(scheduler.state(id), Some(ScriptTaskState::Ready));

        assert_eq!(scheduler.pop_ready(), Some(id));
    }

    #[test]
    fn spawned_waiting_task_wakes_normally() {
        let mut scheduler = ScriptScheduler::new();

        let id = scheduler
            .spawn_waiting(YieldReason::WaitSeconds(1.0))
            .expect("waiting task should spawn");

        assert_eq!(
            scheduler.state(id),
            Some(ScriptTaskState::Waiting { wake_at: 1.0 })
        );

        assert_eq!(scheduler.pop_ready(), None);

        scheduler.tick(1.0).expect("time should advance");

        assert_eq!(scheduler.state(id), Some(ScriptTaskState::Ready));

        assert_eq!(scheduler.pop_ready(), Some(id));
    }

    #[test]
    fn task_can_enter_running_state() {
        let mut scheduler = ScriptScheduler::new();

        let id = scheduler.spawn();

        scheduler.pop_ready().expect("task should be ready");

        scheduler.begin_running(id).expect("task should start");

        assert_eq!(scheduler.state(id), Some(ScriptTaskState::Running));
    }

    #[test]
    fn continue_requeues_task() {
        let mut scheduler = ScriptScheduler::new();

        let id = scheduler.spawn();

        scheduler.pop_ready().expect("task should be ready");

        scheduler.begin_running(id).expect("task should start");

        scheduler
            .apply_result(id, FiberResult::Continue)
            .expect("continue should succeed");

        assert_eq!(scheduler.state(id), Some(ScriptTaskState::Ready));

        assert_eq!(scheduler.pop_ready(), Some(id));
    }

    #[test]
    fn wait_moves_task_to_waiting_state() {
        let mut scheduler = ScriptScheduler::new();

        let id = scheduler.spawn();

        scheduler.pop_ready().expect("task should be ready");

        scheduler.begin_running(id).expect("task should start");

        scheduler
            .apply_result(id, FiberResult::Yield(YieldReason::WaitSeconds(1.0)))
            .expect("wait should succeed");

        assert_eq!(
            scheduler.state(id),
            Some(ScriptTaskState::Waiting { wake_at: 1.0 })
        );

        assert_eq!(scheduler.pop_ready(), None);
    }

    #[test]
    fn wait_does_not_wake_early() {
        let mut scheduler = ScriptScheduler::new();

        let id = scheduler.spawn();

        scheduler.pop_ready().expect("task should be ready");

        scheduler.begin_running(id).expect("task should start");

        scheduler
            .apply_result(id, FiberResult::Yield(YieldReason::WaitSeconds(2.0)))
            .expect("wait should succeed");

        scheduler.tick(1.99).expect("time should advance");

        assert_eq!(
            scheduler.state(id),
            Some(ScriptTaskState::Waiting { wake_at: 2.0 })
        );

        assert_eq!(scheduler.pop_ready(), None);
    }

    #[test]
    fn wait_wakes_at_requested_time() {
        let mut scheduler = ScriptScheduler::new();

        let id = scheduler.spawn();

        scheduler.pop_ready().expect("task should be ready");

        scheduler.begin_running(id).expect("task should start");

        scheduler
            .apply_result(id, FiberResult::Yield(YieldReason::WaitSeconds(2.0)))
            .expect("wait should succeed");

        scheduler.tick(2.0).expect("time should advance");

        assert_eq!(scheduler.state(id), Some(ScriptTaskState::Ready));

        assert_eq!(scheduler.pop_ready(), Some(id));
    }

    #[test]
    fn wait_also_wakes_after_requested_time() {
        let mut scheduler = ScriptScheduler::new();

        let id = scheduler.spawn();

        scheduler.pop_ready().expect("task should be ready");

        scheduler.begin_running(id).expect("task should start");

        scheduler
            .apply_result(id, FiberResult::Yield(YieldReason::WaitSeconds(1.0)))
            .expect("wait should succeed");

        scheduler.tick(5.0).expect("time should advance");

        assert_eq!(scheduler.state(id), Some(ScriptTaskState::Ready));

        assert_eq!(scheduler.pop_ready(), Some(id));
    }

    #[test]
    fn multiple_waiting_tasks_wake() {
        let mut scheduler = ScriptScheduler::new();

        let first = scheduler.spawn();

        let second = scheduler.spawn();

        scheduler.pop_ready().expect("first should be ready");

        scheduler.pop_ready().expect("second should be ready");

        scheduler.begin_running(first).expect("first should start");

        scheduler
            .begin_running(second)
            .expect("second should start");

        scheduler
            .apply_result(first, FiberResult::Yield(YieldReason::WaitSeconds(1.0)))
            .expect("first wait should work");

        scheduler
            .apply_result(second, FiberResult::Yield(YieldReason::WaitSeconds(2.0)))
            .expect("second wait should work");

        scheduler.tick(1.0).expect("time should advance");

        assert_eq!(scheduler.pop_ready(), Some(first));

        assert_eq!(scheduler.pop_ready(), None);

        scheduler.tick(2.0).expect("time should advance");

        assert_eq!(scheduler.pop_ready(), Some(second));
    }

    #[test]
    fn completion_is_terminal() {
        let mut scheduler = ScriptScheduler::new();

        let id = scheduler.spawn();

        scheduler.pop_ready().expect("task should be ready");

        scheduler.begin_running(id).expect("task should start");

        scheduler
            .apply_result(id, FiberResult::Complete)
            .expect("completion should work");

        assert_eq!(scheduler.state(id), Some(ScriptTaskState::Completed));

        assert_eq!(scheduler.pop_ready(), None);
    }

    #[test]
    fn failures_are_terminal() {
        let mut scheduler = ScriptScheduler::new();

        let id = scheduler.spawn();

        scheduler.pop_ready().expect("task should be ready");

        scheduler.begin_running(id).expect("task should start");

        scheduler
            .apply_result(id, FiberResult::Failed("test failure".to_string()))
            .expect("failure should work");

        assert_eq!(scheduler.state(id), Some(ScriptTaskState::Failed));

        assert_eq!(scheduler.pop_ready(), None);
    }

    #[test]
    fn invalid_wait_is_rejected() {
        let mut scheduler = ScriptScheduler::new();

        let id = scheduler.spawn();

        scheduler.pop_ready().expect("task should be ready");

        scheduler.begin_running(id).expect("task should start");

        let result = scheduler.apply_result(id, FiberResult::Yield(YieldReason::WaitSeconds(-1.0)));

        assert!(result.is_err());

        assert_eq!(scheduler.state(id), Some(ScriptTaskState::Running));
    }

    #[test]
    fn non_finite_wait_is_rejected() {
        let mut scheduler = ScriptScheduler::new();

        let id = scheduler.spawn();

        scheduler.pop_ready().expect("task should be ready");

        scheduler.begin_running(id).expect("task should start");

        let result =
            scheduler.apply_result(id, FiberResult::Yield(YieldReason::WaitSeconds(f64::NAN)));

        assert!(result.is_err());

        assert_eq!(scheduler.state(id), Some(ScriptTaskState::Running));
    }

    #[test]
    fn scheduler_rejects_time_rewind() {
        let mut scheduler = ScriptScheduler::new();

        scheduler
            .tick(10.0)
            .expect("initial time advance should work");

        let result = scheduler.tick(9.0);

        assert!(result.is_err());
        assert_eq!(scheduler.current_time(), 10.0);
    }

    #[test]
    fn task_ids_are_unique() {
        let mut scheduler = ScriptScheduler::new();

        let first = scheduler.spawn();

        let second = scheduler.spawn();

        let third = scheduler.spawn();

        assert_ne!(first, second);
        assert_ne!(second, third);
        assert_ne!(first, third);
    }

    #[test]
    fn cancel_marks_task_failed() {
        let mut scheduler = ScriptScheduler::new();

        let id = scheduler.spawn();

        scheduler.cancel(id).expect("cancel should succeed");

        assert_eq!(scheduler.state(id), Some(ScriptTaskState::Failed));
    }

    #[test]
    fn scheduler_remove_purges_task() {
        let mut scheduler = ScriptScheduler::new();

        let id = scheduler.spawn();
        assert_eq!(scheduler.task_count(), 1);

        scheduler.remove(id).expect("remove should succeed");
        assert_eq!(scheduler.task_count(), 0);
        assert_eq!(scheduler.state(id), None);
    }

    #[test]
    fn cancelled_task_removed_from_ready_queue() {
        let mut scheduler = ScriptScheduler::new();

        let id = scheduler.spawn();
        assert_eq!(scheduler.state(id), Some(ScriptTaskState::Ready));

        scheduler.cancel(id).expect("cancel should succeed");

        assert_eq!(scheduler.state(id), Some(ScriptTaskState::Failed));
        assert_eq!(scheduler.pop_ready(), None);
    }
}

// Worldkiln 0.8.7 | WO 0.8.7-17 | 2026-10-06 00:51 -05:00
#[cfg(test)]
mod debugger_pause_scheduler_tests {
    use super::*;

    #[test]
    fn paused_task_is_removed_from_ready_execution_until_explicit_resume() {
        let mut scheduler = ScriptScheduler::new();
        let id = scheduler.spawn();
        assert_eq!(scheduler.pop_ready(), Some(id));
        scheduler.begin_running(id).unwrap();
        scheduler.apply_result(id, FiberResult::Paused).unwrap();
        assert_eq!(scheduler.state(id), Some(ScriptTaskState::Paused));
        assert_eq!(scheduler.pop_ready(), None);

        scheduler.resume_paused(id).unwrap();
        assert_eq!(scheduler.state(id), Some(ScriptTaskState::Ready));
        assert_eq!(scheduler.pop_ready(), Some(id));
    }
}
