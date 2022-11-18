//! Anvil specific [`revm::Inspector`] implementation

use crate::{
    eth::{backend::firehose, macros::node_info},
    revm::{CreateInputs, Database, Interpreter},
};
use bytes::Bytes;
use ethers::types::{Address, Log, H256};
use foundry_evm::{
    call_inspectors,
    decode::decode_console_logs,
    executor::inspector::{LogCollector, Tracer},
    revm,
    revm::{CallInputs, EVMData, Gas, GasInspector, Return},
};
use std::{cell::RefCell, rc::Rc};

/// The [`revm::Inspector`] used when transacting in the evm
#[derive(Debug, Clone, Default)]
pub struct Inspector<'a> {
    pub gas: Option<Rc<RefCell<GasInspector>>>,
    pub tracer: Option<Tracer>,
    /// collects all `console.sol` logs
    pub logs: LogCollector,
    pub firehose: Option<Rc<RefCell<firehose::Inspector<'a>>>>,
}

// === impl Inspector ===

impl<'a> Inspector<'a> {
    /// Called after the inspecting the evm
    ///
    /// This will log all `console.sol` logs
    pub fn print_logs(&self) {
        print_logs(&self.logs.logs)
    }

    /// Configures the `Tracer` [`revm::Inspector`]
    pub fn with_tracing(mut self) -> Self {
        self.tracer = Some(Default::default());
        self
    }

    /// Enables steps recording for `Tracer` and attaches `GasInspector` to it
    /// If `Tracer` wasn't configured before, configures it automatically
    pub fn with_steps_tracing(mut self) -> Self {
        if self.tracer.is_none() {
            self = self.with_tracing()
        }
        let gas_inspector = Rc::new(RefCell::new(GasInspector::default()));
        self.gas = Some(gas_inspector.clone());
        self.tracer = self.tracer.map(|tracer| tracer.with_steps_recording(gas_inspector));

        self
    }

    pub fn with_firehose_tracing(mut self, tracer: &'a mut dyn firehose::Tracer) -> Self {
        self.firehose = tracer.inspector().map(|inspector| Rc::new(RefCell::new(inspector)));

        self
    }
}

impl<'a, DB: Database> revm::Inspector<DB> for Inspector<'a> {
    fn initialize_interp(
        &mut self,
        interp: &mut Interpreter,
        data: &mut EVMData<'_, DB>,
        is_static: bool,
    ) -> Return {
        call_inspectors!(
            inspector,
            [
                &mut self.gas.as_deref().map(|gas| gas.borrow_mut()),
                &mut self.tracer,
                &mut self.firehose.as_deref().map(|inspector| inspector.borrow_mut())
            ],
            { inspector.initialize_interp(interp, data, is_static) }
        );
        Return::Continue
    }

    fn step(
        &mut self,
        interp: &mut Interpreter,
        data: &mut EVMData<'_, DB>,
        is_static: bool,
    ) -> Return {
        call_inspectors!(
            inspector,
            [
                &mut self.gas.as_deref().map(|gas| gas.borrow_mut()),
                &mut self.tracer,
                &mut self.firehose.as_deref().map(|inspector| inspector.borrow_mut())
            ],
            {
                inspector.step(interp, data, is_static);
            }
        );
        Return::Continue
    }

    fn log(
        &mut self,
        evm_data: &mut EVMData<'_, DB>,
        address: &Address,
        topics: &[H256],
        data: &Bytes,
    ) {
        call_inspectors!(
            inspector,
            [
                &mut self.gas.as_deref().map(|gas| gas.borrow_mut()),
                &mut self.tracer,
                Some(&mut self.logs),
                &mut self.firehose.as_deref().map(|inspector| inspector.borrow_mut())
            ],
            {
                inspector.log(evm_data, address, topics, data);
            }
        );
    }

    fn step_end(
        &mut self,
        interp: &mut Interpreter,
        data: &mut EVMData<'_, DB>,
        is_static: bool,
        eval: Return,
    ) -> Return {
        call_inspectors!(
            inspector,
            [
                &mut self.gas.as_deref().map(|gas| gas.borrow_mut()),
                &mut self.tracer,
                &mut self.firehose.as_deref().map(|inspector| inspector.borrow_mut())
            ],
            {
                inspector.step_end(interp, data, is_static, eval);
            }
        );
        eval
    }

    fn call(
        &mut self,
        data: &mut EVMData<'_, DB>,
        call: &mut CallInputs,
        is_static: bool,
    ) -> (Return, Gas, Bytes) {
        call_inspectors!(
            inspector,
            [
                &mut self.gas.as_deref().map(|gas| gas.borrow_mut()),
                &mut self.tracer,
                Some(&mut self.logs),
                &mut self.firehose.as_deref().map(|inspector| inspector.borrow_mut())
            ],
            {
                inspector.call(data, call, is_static);
            }
        );

        (Return::Continue, Gas::new(call.gas_limit), Bytes::new())
    }

    fn call_end(
        &mut self,
        data: &mut EVMData<'_, DB>,
        inputs: &CallInputs,
        remaining_gas: Gas,
        ret: Return,
        out: Bytes,
        is_static: bool,
    ) -> (Return, Gas, Bytes) {
        call_inspectors!(
            inspector,
            [
                &mut self.gas.as_deref().map(|gas| gas.borrow_mut()),
                &mut self.tracer,
                &mut self.firehose.as_deref().map(|inspector| inspector.borrow_mut())
            ],
            {
                inspector.call_end(data, inputs, remaining_gas, ret, out.clone(), is_static);
            }
        );
        (ret, remaining_gas, out)
    }

    fn create(
        &mut self,
        data: &mut EVMData<'_, DB>,
        call: &mut CreateInputs,
    ) -> (Return, Option<Address>, Gas, Bytes) {
        call_inspectors!(
            inspector,
            [
                &mut self.gas.as_deref().map(|gas| gas.borrow_mut()),
                &mut self.tracer,
                &mut self.firehose.as_deref().map(|inspector| inspector.borrow_mut())
            ],
            {
                inspector.create(data, call);
            }
        );

        (Return::Continue, None, Gas::new(call.gas_limit), Bytes::new())
    }

    fn create_end(
        &mut self,
        data: &mut EVMData<'_, DB>,
        inputs: &CreateInputs,
        status: Return,
        address: Option<Address>,
        gas: Gas,
        retdata: Bytes,
    ) -> (Return, Option<Address>, Gas, Bytes) {
        call_inspectors!(
            inspector,
            [
                &mut self.gas.as_deref().map(|gas| gas.borrow_mut()),
                &mut self.tracer,
                &mut self.firehose.as_deref().map(|inspector| inspector.borrow_mut())
            ],
            {
                inspector.create_end(data, inputs, status, address, gas, retdata.clone());
            }
        );
        (status, address, gas, retdata)
    }
}

/// Prints all the logs
pub fn print_logs(logs: &[Log]) {
    for log in decode_console_logs(logs) {
        node_info!("{}", log);
    }
}
