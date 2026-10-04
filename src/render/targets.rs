//! The `artifacts --by-target` view: the incomplete artifacts grouped under
//! the final targets that cannot be made.

use std::collections::BTreeSet;
use std::fmt;

use rustc_hash::{FxHashMap, FxHashSet};

use super::{render_artifact, Report};
use crate::model::{ArtifactInstance, EntityBinding, Gap, IncompleteJob};

/// The indent of a line `depth` levels in, two spaces a level.
fn indent_of(depth: usize) -> String {
    "  ".repeat(depth)
}

impl Report<'_> {
    /// The incomplete artifacts by final target: each one no other incomplete
    /// job needs, then the incomplete artifacts it waits on, nested, each
    /// shown once in the whole report with its own reasons. A reason that
    /// names an incomplete artifact is that artifact's place in the tree
    /// instead, and an artifact already shown is named by a line that says
    /// where: `shown above` under the same target, `shown under <target>`
    /// under an earlier one.
    pub(super) fn write_targets(
        &self,
        f: &mut fmt::Formatter<'_>,
        held_back: &BTreeSet<(&str, &EntityBinding)>,
    ) -> fmt::Result {
        type Key<'k> = (&'k str, &'k EntityBinding);
        /// An incomplete artifact still to be written, or named if an
        /// earlier line already wrote it, and the input that reached it.
        struct Waiting<'r> {
            job: &'r IncompleteJob,
            artifact: &'r ArtifactInstance,
            depth: usize,
            port: Option<&'r str>,
        }
        let dag = &self.report.dag;
        let incomplete = &self.report.incomplete;
        let mut made_by: FxHashMap<Key<'_>, (usize, usize)> = FxHashMap::default();
        let mut needed: FxHashSet<Key<'_>> = FxHashSet::default();
        for (job, incomplete_job) in incomplete.iter().enumerate() {
            for (output, artifact) in incomplete_job.outputs.iter().enumerate() {
                made_by.insert((&artifact.product, &artifact.entities), (job, output));
            }
            for gap in &incomplete_job.gaps {
                if let Gap::Blocked { artifact, .. } = gap {
                    needed.insert((&artifact.product, &artifact.entities));
                }
            }
        }
        let count: usize = incomplete.iter().map(|job| job.outputs.len()).sum();
        let mut targets = Vec::new();
        for job in incomplete {
            for artifact in &job.outputs {
                if !needed.contains(&(artifact.product.as_str(), &artifact.entities)) {
                    targets.push((job, artifact));
                }
            }
        }
        writeln!(
            f,
            "\nFinal targets that cannot be made: {} (incomplete artifacts: {count})",
            targets.len()
        )?;
        // Each artifact written so far, with the number of the target it was
        // written under. It is written when it is reached, not when a job
        // first waits on it, so the first place in the report is the place
        // it is written, and every later reason to name it points back.
        let mut shown: FxHashMap<Key<'_>, usize> = FxHashMap::default();
        for (number, &(job, target)) in targets.iter().enumerate() {
            let mut stack = vec![Waiting {
                job,
                artifact: target,
                depth: 1,
                port: None,
            }];
            while let Some(Waiting {
                job,
                artifact,
                depth,
                port,
            }) = stack.pop()
            {
                let key = (artifact.product.as_str(), &artifact.entities);
                if let (Some(&under), Some(port)) = (shown.get(&key), port) {
                    let name = render_artifact(dag, artifact.view());
                    write!(
                        f,
                        "{}- input `{port}` needs {name}, shown ",
                        indent_of(depth)
                    )?;
                    if under == number {
                        writeln!(f, "above")?;
                    } else {
                        let (_, first) = targets[under];
                        writeln!(f, "under {}", render_artifact(dag, first.view()))?;
                    }
                    continue;
                }
                shown.insert(key, number);
                self.write_output(f, job, artifact, &indent_of(depth))?;
                let indent = indent_of(depth + 1);
                let mut waits = Vec::new();
                for gap in &job.gaps {
                    let Gap::Blocked {
                        port,
                        artifact: input,
                    } = gap
                    else {
                        self.write_gap(f, gap, held_back, &indent)?;
                        continue;
                    };
                    let key = (input.product.as_str(), &input.entities);
                    match made_by.get(&key) {
                        Some(&(inner, output)) => {
                            let inner = &incomplete[inner];
                            waits.push(Waiting {
                                job: inner,
                                artifact: &inner.outputs[output],
                                depth: depth + 1,
                                port: Some(port),
                            });
                        }
                        None => self.write_gap(f, gap, held_back, &indent)?,
                    }
                }
                stack.extend(waits.into_iter().rev());
            }
        }
        Ok(())
    }
}
