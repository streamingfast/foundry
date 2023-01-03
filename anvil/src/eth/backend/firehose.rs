use std::fmt::Display;

use anvil_core::eth::{
    block::{Block, Header},
    receipt::{Log, TypedReceipt},
    transaction::TransactionInfo,
};
use ethers::{
    abi::Address,
    types::{Bloom, Bytes, H160, H256, H64, U256},
    utils::rlp::RlpStream,
};
use forge::revm::{self, CallScheme, CreateScheme, Database, TxEnv};

use crate::eth::pool::transactions::PoolTransaction;

pub fn new_tracer(enabled: bool) -> Box<dyn Tracer> {
    if enabled {
        Box::new(PrinterTracer { ..Default::default() })
    } else {
        Box::new(NoOpTracer {})
    }
}

pub struct BigInt<'a>(&'a U256);

impl<'a> std::fmt::Display for BigInt<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.0.is_zero() {
            return f.write_str(".");
        }

        let mut bytes = [0u8; 32];
        self.0.to_big_endian(&mut bytes);

        Display::fmt(&Hex(&bytes), f)
    }
}

pub struct Hex<'a>(&'a [u8]);

impl<'a> std::fmt::Display for Hex<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.0.len() == 0 {
            return f.write_str(".");
        }

        let mut is_all_zero = true;
        for byte in self.0 {
            if *byte != 0u8 {
                is_all_zero = false;
                break;
            }
        }

        if is_all_zero {
            return f.write_str(".");
        }

        for byte in self.0 {
            f.write_fmt(format_args!("{byte:02x}"))?;
        }

        Ok(())
    }
}

pub trait Tracer: std::fmt::Debug + Send + Sync {
    fn enabled(&self) -> bool {
        return false;
    }
    fn record_init(&self) {}
    fn start_block(&mut self, _block_number: u64) {}
    fn finalize_block(&mut self, _block_number: u64) {}
    fn end_block(&mut self, _block: &Block, _total_difficulty: U256) {}

    fn start_transaction(&mut self, _tx: &PoolTransaction) {}
    fn record_start_transaction(&mut self, _tx: &TxEnv) {}
    fn end_transaction(
        &mut self,
        _cumulative_gas_used: U256,
        _tx: &TransactionInfo,
        _receipt: &TypedReceipt,
    ) {
    }

    fn start_call(&mut self, _call_type: CallType) {}
    fn record_call_params(
        &mut self,
        _call_type: CallType,
        _from: &H160,
        _to: Option<&H160>,
        _value: &U256,
        _gas_limit: u64,
        _input: &bytes::Bytes,
    ) {
    }
    fn end_call(&mut self, _gas_left: u64, _value: &bytes::Bytes) {}

    fn record_log(&mut self, _address: &H160, _topics: &[H256], _data: &bytes::Bytes) {}

    fn inspector<'a>(&'a mut self) -> Option<Inspector<'a>> {
        None
    }
}

#[derive(Debug)]
pub struct NoOpTracer {}

impl Tracer for NoOpTracer {}

#[derive(Debug, Default)]
pub struct PrinterTracer {
    block_log_index: u64,
    total_ordering: u64,
    active_tx_data: Option<TxData>,
    active_tx_recorded: bool,

    call_index_counter: u64,
    active_call_stack: Vec<u64>,
    active_call_index: u64,
}

impl Tracer for PrinterTracer {
    fn enabled(&self) -> bool {
        return true;
    }

    fn record_init(&self) {
        // FIXME: 2.1 Should be defined somewhere as the Firehose instrumentation version
        // FIXME: 1.0 should be replaced with actual Anvil's version (extract from Cargo.toml most probably)
        eprintln!("FIRE INIT 2.1 anvil 1.0")
    }

    fn start_block(&mut self, block_number: u64) {
        self.total_ordering = 0;
        eprintln!("FIRE BEGIN_BLOCK {}", block_number);
    }

    fn finalize_block(&mut self, block_number: u64) {
        self.total_ordering = 0;
        eprintln!("FIRE FINALIZE_BLOCK {}", block_number);
    }

    fn end_block(&mut self, block: &Block, total_difficulty: U256) {
        self.exit_block();

        let mut rlp_stream = RlpStream::new();
        rlp_stream.append(block);

        let block_number = block.header.number;
        let block_size = rlp_stream.len();
        let block_header = FirehoseHeader::from_header(&block.header);
        let end_block = EndBlockData {
            header: block_header,
            total_difficulty: &total_difficulty,
            uncles: &block.ommers,
        };

        let end_block_data = serde_json::to_string(&end_block)
            .expect("should have been enable to serialize end block data");

        eprintln!("FIRE END_BLOCK {} {} {}", block_number, block_size, end_block_data);
    }

    fn start_transaction(&mut self, tx: &PoolTransaction) {
        if self.active_tx_data.is_some() {
            panic!("a transaction is already active in this tracer, this is invalid")
        }

        self.active_tx_data = Some(tx.into());
        self.active_call_stack = vec![0];
        self.active_call_index = 0;
        self.call_index_counter = 0;
    }

    fn record_start_transaction(&mut self, tx: &TxEnv) {
        if self.active_tx_data.is_none() {
            panic!("no active transaction on this tracer, this is invalid")
        }

        if self.active_tx_recorded {
            return;
        }

        let tx_data = self
            .active_tx_data
            .as_ref()
            .expect("no active transaction on this tracer, this is invalid");

        let to = match tx.transact_to {
            revm::TransactTo::Call(address) => format!("{:x}", address),
            revm::TransactTo::Create(_) => ".".to_string(),
        };

        eprintln!("FIRE BEGIN_APPLY_TRX {hash:x} {to} {value:x} {v} {r} {s} {gas_limit} {gas_price:x} {nonce} {data} {access_list} {max_fee_per_gas} {max_priority_fee_per_gas} {tx_type} {total_ordering}",
            hash = tx_data.hash,
            to = to,
            value = tx.value,
            v = tx_data.v,
            r = Hex(tx_data.r.as_bytes()),
            s = Hex(tx_data.s.as_bytes()),
            gas_limit = tx.gas_limit,
            gas_price = tx.gas_price,
            nonce = tx.nonce.unwrap_or(0),
            data = Hex(&tx.data),
            // TODO: AccessList encoding to Firehose format
            access_list = "00",
            max_fee_per_gas = tx_data.max_fee_per_gas.as_ref().map(ToString::to_string).unwrap_or(".".to_string()),
            max_priority_fee_per_gas = tx_data.max_priority_fee_per_gas.as_ref().map(ToString::to_string).unwrap_or(".".to_string()),
            tx_type = tx_data.tx_type.as_u8(),
            // FIXME: Implement total ordering
            total_ordering = 0,
        );

        eprintln!("FIRE TRX_FROM {from:x}",
            from = tx.caller,
        );

        self.active_tx_recorded = true;

        //         Hash(hash),
        //         toAsString,
        //         Hex(value.Bytes()),
        //         Hex(v),
        //         Hex(r),
        //         Hex(s),
        //         Uint64(gasLimit),
        //         Hex(gasPrice.Bytes()),
        //         Uint64(nonce),
        //         Hex(data),
        //         Hex(accessList.marshal()),
        //         maxFeePerGasAsString,
        //         maxPriorityFeePerGasAsString,
        //         Uint8(txType),
        //         Uint64(ctx.totalOrderingCounter.Inc()),

        // func (ctx *Context) StartTransaction(tx *types.Transaction, baseFee *big.Int) {
        //     hash := tx.Hash()
        //     v, r, s := tx.RawSignatureValues()

        //     ctx.StartTransactionRaw(
        //         hash,
        //         tx.To(),
        //         tx.Value(),
        //         v.Bytes(),
        //         r.Bytes(),
        //         s.Bytes(),
        //         tx.Gas(),
        //         gasPrice(tx, baseFee),
        //         tx.Nonce(),
        //         tx.Data(),
        //         AccessList(tx.AccessList()),
        //         maxFeePerGas(tx),
        //         maxPriorityFeePerGas(tx),
        //         tx.Type(),
        //     )
        // func (ctx *Context) StartTransactionRaw(
        //     hash common.Hash,
        //     to *common.Address,
        //     value *big.Int,
        //     v, r, s []byte,
        //     gasLimit uint64,
        //     gasPrice *big.Int,
        //     nonce uint64,
        //     data []byte,
        //     accessList AccessList,
        //     maxFeePerGas *big.Int,
        //     maxPriorityFeePerGas *big.Int,
        //     txType uint8,
        // ) {
        //     if ctx == nil {
        //         return
        //     }

        //     ctx.openTransaction()

        //     // We start assuming the "null" value (i.e. a dot character), and update if `to` is set
        //     toAsString := "."
        //     if to != nil {
        //         toAsString = Addr(*to)
        //     }

        //     maxFeePerGasAsString := "."
        //     if maxFeePerGas != nil {
        //         maxFeePerGasAsString = Hex(maxFeePerGas.Bytes())
        //     }

        //     maxPriorityFeePerGasAsString := "."
        //     if maxPriorityFeePerGas != nil {
        //         maxPriorityFeePerGasAsString = Hex(maxPriorityFeePerGas.Bytes())
        //     }

        //     ctx.printer.Print("BEGIN_APPLY_TRX",
        //         Hash(hash),
        //         toAsString,
        //         Hex(value.Bytes()),
        //         Hex(v),
        //         Hex(r),
        //         Hex(s),
        //         Uint64(gasLimit),
        //         Hex(gasPrice.Bytes()),
        //         Uint64(nonce),
        //         Hex(data),
        //         Hex(accessList.marshal()),
        //         maxFeePerGasAsString,
        //         maxPriorityFeePerGasAsString,
        //         Uint8(txType),
        //         Uint64(ctx.totalOrderingCounter.Inc()),
        //     )
        // }
    }

    fn end_transaction(
        &mut self,
        cumulative_gas_used: U256,
        _tx: &TransactionInfo,
        receipt: &TypedReceipt,
    ) {
        if self.active_tx_data.is_none() {
            panic!("no active transaction on this tracer, this is invalid")
        }

        //     logItems := make([]logItem, len(receipt.Logs))
        //     for i, log := range receipt.Logs {
        //         logItems[i] = logItem{
        //             "address": log.Address,
        //             "topics":  log.Topics,
        //             "data":    hexutil.Bytes(log.Data),
        //         }
        //     }

        let logs: Vec<_> = receipt
            .logs()
            .iter()
            .map(|log| FirehoseLogItem { address: &log.address, data: &log.data })
            .collect();

        let logs_data =
            serde_json::to_string(&logs).expect("should have been enable to serialize logs data");

        eprintln!("FIRE END_APPLY_TRX {gas_used} {post_state} {cumulative_gas_used} {bloom:x} {total_ordering} {logs}",
            gas_used = receipt.gas_used(),
            // FIXME: Implement post_state retrieval
            post_state = ".",
            cumulative_gas_used = cumulative_gas_used,
            bloom = receipt.logs_bloom(),
            // FIXME: Implement total ordering
            total_ordering = 0,
            logs = logs_data,
        );

        self.active_tx_data = None;
        self.active_tx_recorded = false;

        //     ctx.printer.Print(
        //         "END_APPLY_TRX",
        //         Uint64(receipt.GasUsed),
        //         Hex(receipt.PostState),
        //         Uint64(receipt.CumulativeGasUsed),
        //         Hex(receipt.Bloom[:]),
        //         Uint64(ctx.totalOrderingCounter.Inc()),
        //         JSON(logItems),
        //     )

        //     ctx.nextCallIndex = 0
        //     ctx.activeCallIndex = "0"
        //     ctx.callIndexStack = &ExtendedStack{}
        //     ctx.callIndexStack.Push(ctx.activeCallIndex)
        // }
    }

    fn start_call(&mut self, call_type: CallType) {
        if self.active_tx_data.is_none() {
            panic!("no active transaction on this tracer, this is invalid")
        }

        self.call_index_counter += 1;
        self.active_call_index = self.call_index_counter;
        self.active_call_stack.push(self.call_index_counter);

        eprintln!(
            "FIRE EVM_RUN_CALL {call_type} {call_index} {total_ordering}",
            call_type = call_type,
            call_index = self.call_index_counter,
            total_ordering = 0
        );

        // func (ctx *Context) StartCall(callType string) {
        //     if ctx == nil {
        //         return
        //     }

        //     ctx.printer.Print("EVM_RUN_CALL",
        //         callType,
        //         ctx.openCall(),
        //         Uint64(ctx.totalOrderingCounter.Inc()),
        //     )

        // }
    }

    fn record_call_params(
        &mut self,
        call_type: CallType,
        from: &H160,
        to: Option<&H160>,
        value: &U256,
        gas_limit: u64,
        input: &bytes::Bytes,
    ) {
        if self.active_tx_data.is_none() {
            panic!("no active transaction on this tracer, this is invalid")
        }

        eprintln!(
            "FIRE EVM_PARAM {call_type} {call_index} {from:x} {to} {value} {gas_limit} {input}",
            call_type = call_type,
            call_index = self.active_call_index,
            from = from,
            to = to.map(|x| format!("{:x}", x)).unwrap_or(".".to_string()),
            value = BigInt(&value),
            gas_limit = gas_limit,
            input = Hex(input)
        );

        // func (ctx *Context) RecordCallParams(callType string, caller common.Address, callee common.Address, value *big.Int, gasLimit uint64, input []byte) {
        //     if ctx == nil {
        //         return
        //     }

        //     ctx.printer.Print("EVM_PARAM",
        //         callType,
        //         ctx.callIndex(),
        //         Addr(caller),
        //         Addr(callee),
        //         Hex(value.Bytes()),
        //         Uint64(gasLimit),
        //         Hex(input),
        //     )
        // }
    }

    fn end_call(&mut self, gas_left: u64, return_value: &bytes::Bytes) {
        if self.active_tx_data.is_none() {
            panic!("no active transaction on this tracer, this is invalid")
        }

        let previous_index = self
            .active_call_stack
            .pop()
            .expect("There is no more call in the stack, that is unexpected");
        self.active_call_index = self
            .active_call_stack
            .last()
            .expect("There is no more call in the stack, that is unexpected")
            .clone();

        eprintln!(
            "FIRE EVM_END_CALL {call_index} {gas_left} {return_value} {total_ordering}",
            call_index = previous_index,
            gas_left = gas_left,
            return_value = Hex(return_value),
            total_ordering = 0,
        );

        // func (ctx *Context) EndCall(gasLeft uint64, returnValue []byte) {
        //     if ctx == nil {
        //         return
        //     }

        //     ctx.printer.Print("EVM_END_CALL",
        //         ctx.closeCall(),
        //         Uint64(gasLeft),
        //         Hex(returnValue),
        //         Uint64(ctx.totalOrderingCounter.Inc()),
        //     )
        // }
    }

    fn record_log(&mut self, address: &H160, topics: &[H256], data: &bytes::Bytes) {
        // func (ctx *Context) RecordLog(log *types.Log) {
        //     if ctx == nil {
        //         return
        //     }

        //     strtopics := make([]string, len(log.Topics))
        //     for idx, topic := range log.Topics {
        //         strtopics[idx] = Hash(topic)
        //     }

        //     ctx.printer.Print("ADD_LOG",
        //         ctx.callIndex(),
        //         ctx.logIndexInBlock(),
        //         Addr(log.Address),
        //         strings.Join(strtopics, ","),
        //         Hex(log.Data),
        //         Uint64(ctx.totalOrderingCounter.Inc()),
        //     )
        // }

        let topics_strings: Vec<String> =
            topics.iter().map(|topic| format!("{}", Hex(topic.as_bytes()))).collect();

        eprintln!("FIRE ADD_LOG {call_index} {block_log_index} {address:x} {topics} {data} {total_ordering}",
            call_index = self.active_call_index,
            // Still logged but unsued on the reader side, re-computed there because we have some logs here that will be reverted, so block index
            // will be known only at a later point
            block_log_index = 0,
            address = address,
            topics = topics_strings.join(","),
            data = Hex(data),
            total_ordering = 0,
        );
    }

    fn inspector<'a>(&'a mut self) -> Option<Inspector<'a>> {
        Some(Inspector { tracer: self })
    }
}

#[derive(Debug, Default)]
enum TxType {
    #[default]
    Legacy,
    AccessList,
    DynamicFee,
}

impl TxType {
    fn as_u8(&self) -> u8 {
        match self {
            TxType::Legacy => 0,
            TxType::AccessList => 1,
            TxType::DynamicFee => 2,
        }
    }
}

#[derive(Debug, Default)]
struct TxData {
    tx_type: TxType,
    hash: H256,
    max_fee_per_gas: Option<U256>,
    max_priority_fee_per_gas: Option<U256>,
    v: u64,
    r: H256,
    s: H256,
}

impl Into<TxData> for &PoolTransaction {
    fn into(self) -> TxData {
        use anvil_core::eth::transaction::TypedTransaction;

        let (tx_type, max_fee_per_gas, max_priority_fee_per_gas, v, r, s) =
            match &self.pending_transaction.transaction {
                TypedTransaction::Legacy(tx) => {
                    let sig = tx.signature;

                    let mut r_bytes = [0u8; 32];
                    sig.r.to_big_endian(&mut r_bytes);

                    let mut s_bytes = [0u8; 32];
                    sig.s.to_big_endian(&mut s_bytes);

                    (TxType::Legacy, None, None, sig.v, r_bytes.into(), s_bytes.into())
                }
                TypedTransaction::EIP2930(tx) => {
                    (TxType::AccessList, None, None, tx.odd_y_parity as u64, tx.r, tx.s)
                }
                TypedTransaction::EIP1559(tx) => (
                    TxType::DynamicFee,
                    Some(tx.max_fee_per_gas),
                    Some(tx.max_priority_fee_per_gas),
                    tx.odd_y_parity as u64,
                    tx.r,
                    tx.s,
                ),
            };

        TxData {
            hash: self.hash().clone(),
            tx_type,
            max_fee_per_gas,
            max_priority_fee_per_gas,
            v,
            r,
            s,
        }
    }
}

#[derive(serde::Serialize)]
struct EndBlockData<'a> {
    #[serde(rename = "header")]
    header: FirehoseHeader<'a>,

    #[serde(rename = "uncles")]
    uncles: &'a Vec<Header>,

    #[serde(rename = "totalDifficulty")]
    total_difficulty: &'a U256,
}

#[derive(serde::Serialize)]
pub struct FirehoseLogItem<'a> {
    #[serde(rename = "address")]
    pub address: &'a Address,

    #[serde(rename = "data")]
    pub data: &'a Bytes,
}

impl<'a> Into<FirehoseLogItem<'a>> for &'a Log {
    fn into(self) -> FirehoseLogItem<'a> {
        FirehoseLogItem { address: &self.address, data: &self.data }
    }
}

//     logItems := make([]logItem, len(receipt.Logs))
//     for i, log := range receipt.Logs {
//         logItems[i] = logItem{
//             "address": log.Address,
//             "topics":  log.Topics,
//             "data":    hexutil.Bytes(log.Data),
//         }
//     }

#[derive(serde::Serialize)]
pub struct FirehoseHeader<'a> {
    #[serde(rename = "hash")]
    pub hash: H256,
    #[serde(rename = "parentHash")]
    pub parent_hash: &'a H256,
    #[serde(rename = "sha3Uncles")]
    pub ommers_hash: &'a H256,
    #[serde(rename = "miner")]
    pub beneficiary: &'a Address,
    #[serde(rename = "stateRoot")]
    pub state_root: &'a H256,
    #[serde(rename = "transactionsRoot")]
    pub transactions_root: &'a H256,
    #[serde(rename = "receiptsRoot")]
    pub receipts_root: &'a H256,
    #[serde(rename = "logsBloom")]
    pub logs_bloom: &'a Bloom,
    #[serde(rename = "difficulty")]
    pub difficulty: &'a U256,
    #[serde(rename = "number")]
    pub number: &'a U256,
    #[serde(rename = "gasLimit")]
    pub gas_limit: &'a U256,
    #[serde(rename = "gasUsed")]
    pub gas_used: &'a U256,
    #[serde(rename = "timestamp")]
    pub timestamp: H64,
    #[serde(rename = "extraData")]
    pub extra_data: &'a Bytes,
    #[serde(rename = "mixHash")]
    pub mix_hash: &'a H256,
    #[serde(rename = "nonce")]
    pub nonce: &'a H64,
    #[serde(rename = "baseFeePerGas")]
    pub base_fee_per_gas: &'a Option<U256>,
}

impl<'a> FirehoseHeader<'a> {
    fn from_header(header: &'a Header) -> FirehoseHeader<'a> {
        FirehoseHeader {
            hash: header.hash(),
            parent_hash: &header.parent_hash,
            ommers_hash: &header.ommers_hash,
            beneficiary: &header.beneficiary,
            state_root: &header.state_root,
            transactions_root: &header.transactions_root,
            receipts_root: &header.receipts_root,
            logs_bloom: &header.logs_bloom,
            difficulty: &header.difficulty,
            number: &header.number,
            gas_limit: &header.gas_limit,
            gas_used: &header.gas_used,
            timestamp: H64::from_low_u64_be(header.timestamp),
            extra_data: &header.extra_data,
            mix_hash: &header.mix_hash,
            nonce: &header.nonce,
            base_fee_per_gas: &header.base_fee_per_gas,
        }
    }
}

impl PrinterTracer {
    fn exit_block(&mut self) {
        self.block_log_index = 0;
    }
}

#[derive(Debug)]
pub struct Inspector<'a> {
    tracer: &'a mut dyn Tracer,
}

impl<'a, DB: Database> revm::Inspector<DB> for Inspector<'a> {
    fn initialize_interp(
        &mut self,
        _interp: &mut revm::Interpreter,
        _data: &mut revm::EVMData<'_, DB>,
        _is_static: bool,
    ) -> revm::Return {
        revm::Return::Continue
    }

    fn step(
        &mut self,
        _interp: &mut revm::Interpreter,
        _data: &mut revm::EVMData<'_, DB>,
        _is_static: bool,
    ) -> revm::Return {
        revm::Return::Continue
    }

    fn log(
        &mut self,
        _evm_data: &mut revm::EVMData<'_, DB>,
        address: &ethers::types::H160,
        topics: &[H256],
        data: &bytes::Bytes,
    ) {
        self.tracer.record_log(address, topics, data);
    }

    fn step_end(
        &mut self,
        _interp: &mut revm::Interpreter,
        _data: &mut revm::EVMData<'_, DB>,
        _is_static: bool,
        _eval: revm::Return,
    ) -> revm::Return {
        revm::Return::Continue
    }

    fn call(
        &mut self,
        data: &mut revm::EVMData<'_, DB>,
        inputs: &mut revm::CallInputs,
        _is_static: bool,
    ) -> (revm::Return, revm::Gas, bytes::Bytes) {
        // Done conditionally if never done yet
        self.tracer.record_start_transaction(&data.env.tx);

        self.tracer.start_call((&inputs.context.scheme).into());
        self.tracer.record_call_params(
            (&inputs.context.scheme).into(),
            &inputs.context.caller,
            Some(&inputs.contract),
            &inputs.transfer.value,
            inputs.gas_limit,
            &inputs.input,
        );

        (revm::Return::Continue, revm::Gas::new(0), bytes::Bytes::new())
    }

    fn call_end(
        &mut self,
        _data: &mut revm::EVMData<'_, DB>,
        _inputs: &revm::CallInputs,
        remaining_gas: revm::Gas,
        ret: revm::Return,
        out: bytes::Bytes,
        _is_static: bool,
    ) -> (revm::Return, revm::Gas, bytes::Bytes) {
        self.tracer.end_call(remaining_gas.remaining(), &out);

        (ret, remaining_gas, out)
    }

    fn create(
        &mut self,
        data: &mut revm::EVMData<'_, DB>,
        inputs: &mut revm::CreateInputs,
    ) -> (revm::Return, Option<ethers::types::H160>, revm::Gas, bytes::Bytes) {
        // Done conditionally if never done yet
        self.tracer.record_start_transaction(&data.env.tx);

        self.tracer.start_call((&inputs.scheme).into());
        (revm::Return::Continue, None, revm::Gas::new(0), bytes::Bytes::default())
    }

    fn create_end(
        &mut self,
        _data: &mut revm::EVMData<'_, DB>,
        _inputs: &revm::CreateInputs,
        ret: revm::Return,
        address: Option<ethers::types::H160>,
        remaining_gas: revm::Gas,
        out: bytes::Bytes,
    ) -> (revm::Return, Option<ethers::types::H160>, revm::Gas, bytes::Bytes) {
        self.tracer.end_call(remaining_gas.remaining(), &out);
        (ret, address, remaining_gas, out)
    }

    fn selfdestruct(&mut self) {}
}

pub enum CallType {
    CALL,
    CREATE,
    CREATE2,
    CALLCODE,
    STATICCALL,
    DELEGATECALL,
}

impl Into<CallType> for &ethers::types::CallType {
    fn into(self) -> CallType {
        match self {
            ethers::types::CallType::None => panic!("Not accepting CallType::None"),
            ethers::types::CallType::Call => CallType::CALL,
            ethers::types::CallType::CallCode => CallType::CALLCODE,
            ethers::types::CallType::DelegateCall => CallType::DELEGATECALL,
            ethers::types::CallType::StaticCall => CallType::STATICCALL,
        }
    }
}

impl Into<CallType> for &CallScheme {
    fn into(self) -> CallType {
        match self {
            CallScheme::Call => CallType::CALL,
            CallScheme::CallCode => CallType::CALLCODE,
            CallScheme::DelegateCall => CallType::DELEGATECALL,
            CallScheme::StaticCall => CallType::STATICCALL,
        }
    }
}

impl Into<CallType> for &CreateScheme {
    fn into(self) -> CallType {
        match self {
            CreateScheme::Create => CallType::CREATE,
            CreateScheme::Create2 { salt: _ } => CallType::CREATE2,
        }
    }
}

impl Display for CallType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let value = match self {
            CallType::CALL => "CALL",
            CallType::CREATE => "CREATE",
            CallType::CREATE2 => "CREATE",
            CallType::CALLCODE => "CALLCODE",
            CallType::STATICCALL => "STATIC",
            CallType::DELEGATECALL => "DELEGATE",
        };

        f.write_str(value)
    }
}
