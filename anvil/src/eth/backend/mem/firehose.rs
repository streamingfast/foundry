use anvil_core::eth::block::{Block, Header};
use ethers::{
    abi::Address,
    types::{Bloom, Bytes, H256, H64, U256},
    utils::rlp::RlpStream,
};

pub fn new_tracer(enabled: bool) -> Box<dyn Tracer> {
    if enabled {
        Box::new(PrinterTracer { ..Default::default() })
    } else {
        Box::new(NoOpTracer {})
    }
}

pub trait Tracer: Send + Sync {
    fn enabled(&self) -> bool {
        return false;
    }
    fn record_init(&self) {}
    fn start_block(&mut self, _block_number: u64) {}
    fn finalize_block(&mut self, _block_number: u64) {}
    fn end_block(&mut self, _block: &Block, _total_difficulty: U256) {}
}

pub struct NoOpTracer {}

impl Tracer for NoOpTracer {}

#[derive(Default)]
pub struct PrinterTracer {
    block_log_index: u64,
    total_ordering: u64,
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

    // func (ctx *Context) EndBlock(block *types.Block, finalizedBlock *types.Block, totalDifficulty *big.Int) {
    //     ctx.ExitBlock()

    //     endData := map[string]interface{}{
    //         "header":          block.Header(),
    //         "uncles":          block.Body().Uncles,
    //         "totalDifficulty": (*hexutil.Big)(totalDifficulty),
    //     }

    //     if finalizedBlock != nil {
    //         endData["finalizedBlockNum"] = (*hexutil.Big)(finalizedBlock.Header().Number)
    //         endData["finalizedBlockHash"] = finalizedBlock.Header().Hash()
    //     }

    //     ctx.printer.Print("END_BLOCK",
    //         Uint64(block.NumberU64()),
    //         Uint64(uint64(block.Size())),
    //         JSON(endData),
    //     )
    // }

    // func (ctx *Context) RecordGenesisBlock(block *types.Block, recordGenesisAlloc func(ctx *Context)) {
    //     if ctx == nil {
    //         return
    //     }

    //     if ctx.inBlock.Load() == true {
    //         panic("trying to record genesis block while in block context")
    //     }

    //     zero := common.Address{}
    //     root := block.Root()

    //     ctx.StartBlock(block)
    //     ctx.StartTransactionRaw(common.Hash{}, &zero, &big.Int{}, nil, nil, nil, 0, &big.Int{}, 0, nil, nil, nil, nil, 0)
    //     ctx.RecordTrxFrom(zero)
    //     recordGenesisAlloc(ctx)
    //     ctx.EndTransaction(&types.Receipt{PostState: root[:]})
    //     ctx.FinalizeBlock(block)
    //     ctx.EndBlock(block, nil, block.Difficulty())
    // }
    // }
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
