use std::{io::Read, sync::Arc};

use eyre::Result;
use tokio::runtime::Handle;
use virtual_fs::Pipe;
use wasmer::{
    imports, EngineBuilder, Extern, Features, Function, FunctionType, Instance, Module, Store,
    Type, Value,
};
use wasmer_compiler_cranelift::Cranelift;
use wasmer_wasix::{
    capabilities::{Capabilities, CapabilityThreadingV1},
    generate_import_object_from_env,
    http::HttpClientCapabilityV1,
    PluggableRuntime, WasiEnv, WasiEnvBuilder,
};

fn create_wasi_env(
    capabilities: Capabilities,
    runtime_handle: tokio::runtime::Handle,
) -> eyre::Result<(WasiEnvBuilder, Pipe, Pipe, Pipe)> {
    let (stdin_tx, stdin_rx) = Pipe::channel();
    let (stdout, stdout_rx) = Pipe::channel();
    let (stderr, stderr_rx) = Pipe::channel();

    let runtime = PluggableRuntime::new(Arc::new(
        wasmer_wasix::runtime::task_manager::tokio::TokioTaskManager::new(runtime_handle),
    ));

    let builder = WasiEnv::builder("nor2")
        .runtime(Arc::new(runtime))
        .capabilities(capabilities)
        .stdin(Box::new(stdin_rx))
        .stdout(Box::new(stdout))
        .stderr(Box::new(stderr));

    Ok((builder, stdin_tx, stdout_rx, stderr_rx))
}

fn main() -> Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    let handle = runtime.handle().clone();

    start(handle)?;

    Ok(())
}

fn start(handle: Handle) -> Result<()> {
    let mut features = Features::default();
    features.reference_types(true);
    features.multi_memory(true);
    features.module_linking(true);
    features.tail_call(true);
    features.threads(true);

    let engine = EngineBuilder::new(Cranelift::default())
        .set_features(Some(features))
        .engine();

    let capabilities = Capabilities {
        insecure_allow_all: true,
        http_client: HttpClientCapabilityV1::new_allow_all(),
        threading: CapabilityThreadingV1::default(),
    };

    let (builder, _stdin_tx, mut stdout_rx, mut stderr_rx) = create_wasi_env(capabilities, handle)?;

    let mut store = Store::new(engine);
    let module = Module::new(&store, include_bytes!("../TestGenerated.wasm"))?;

    let mut wasi_env = builder.finalize(&mut store)?;

    let imports = imports! {
      "Integers" => {
        "a6: func(x: s32) -> ()" => Function::new(&mut store, FunctionType::new(vec![Type::I32], vec![]), |args| {
            println!("a6: Hello from rust {args:#?}");
            Ok(vec![])
        })
      },
      "strings" => {
        "a: func(x: string) -> ()" => Function::new(&mut store, FunctionType::new(vec![Type::I32, Type::I32], vec![]), |args| {
            println!("a: Hello from rust {args:#?}");
            Ok(vec![])
        }),
        "b: func() -> string" => Function::new(&mut store, FunctionType::new(vec![Type::I32], vec![]), |args| {
            println!("b: Hello from rust {args:#?}");
            Ok(vec![])
        }),
        "c: func(a: string, b: string) -> string" => Function::new(&mut store, FunctionType::new(vec![Type::I32, Type::I32, Type::I32, Type::I32, Type::I32], vec![]), |args| {
            println!("c: Hello from rust {args:#?}");
            Ok(vec![])
        })
      },
      "rust" => {
        "wasmImportFloat32Param" => Function::new(&mut store, FunctionType::new(vec![Type::F32], vec![]), |args| {
            println!("Hello from rust {args:#?}");
            Ok(vec![])
        })
      },
      "wasi" => {
        "thread-spawn" => Function::new(&mut store, FunctionType::new(vec![Type::I32], vec![Type::I32]), |args| {
            println!("Hello from rust {args:#?}");
            Ok(vec![Value::I32(0)])
        })
      }
    };
    let extend: Vec<((String, String), Extern)> = imports.into_iter().collect();

    let mut import_object = wasi_env.import_object_for_all_wasi_versions(&mut store, &module)?;
    import_object.extend(extend);

    let instance = Instance::new(&mut store, &module, &import_object)?;

    wasi_env.data(&store).thread.set_status_running();

    wasi_env.initialize(&mut store, instance.clone())?;

    let start_func = instance.exports.get_function("_start").unwrap();

    let result = start_func.call(&mut store, &[]);

    wasi_env.cleanup(&mut store, None);

    let mut std_out = String::default();
    if stdout_rx.read_to_string(&mut std_out)? != 0 {
        println!("Std out: {std_out}");
    }

    let mut std_err = String::default();
    if stderr_rx.read_to_string(&mut std_err)? != 0 {
        println!("Std err: {std_err}");
    }

    match result {
        Ok(_) => println!("Success"),
        Err(err) => {
            println!("Runtime Error: {err}");
        }
    }

    Ok(())
}
