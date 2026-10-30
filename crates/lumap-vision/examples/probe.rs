//! Temporary probe: prints model facts and raw inference time. Not committed.
#![allow(missing_docs, clippy::unwrap_used, clippy::print_stdout)]

use std::time::Instant;
use tract_linalg::multithread::Executor;
use tract_onnx::prelude::*;

fn probe(path: &str, shape: [usize; 4], threads: usize) {
    let raw = onnx().model_for_path(path).unwrap();
    let model = raw
        .with_input_fact(0, f32::fact(shape).into())
        .unwrap()
        .into_optimized()
        .unwrap();
    let options = RunOptions {
        executor: (threads > 1).then(|| Executor::multithread(threads)),
        ..RunOptions::default()
    };
    let plan = model.into_runnable_with_options(&options).unwrap();
    let mut state = plan.spawn().unwrap();
    let input: Arc<Tensor> = Arc::new(Tensor::zero::<f32>(&shape).unwrap());
    for _ in 0..3 {
        let _ = state
            .run(tvec!(TValue::from_const(Arc::clone(&input))))
            .unwrap();
    }
    let n = 20;
    let t = Instant::now();
    for _ in 0..n {
        let out = state
            .run(tvec!(TValue::from_const(Arc::clone(&input))))
            .unwrap();
        assert!(!out.is_empty());
    }
    println!(
        "{path} threads={threads}: {:?} per run",
        t.elapsed() / n
    );
}

fn main() {
    let dir = std::env::args().nth(1).unwrap();
    for threads in [1, 2, 4] {
        probe(
            &format!("{dir}/palm_detection_mediapipe_2023feb.onnx"),
            [1, 192, 192, 3],
            threads,
        );
        probe(
            &format!("{dir}/handpose_estimation_mediapipe_2023feb.onnx"),
            [1, 224, 224, 3],
            threads,
        );
    }
}
