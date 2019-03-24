use std::io::Write;

use bitcoin::consensus::encode::serialize;
use bitcoin::{Network, OutPoint, Script, Transaction, TxIn, TxOut};

use cmd;
use hal::tx::{InputInfo, InputScriptInfo, OutputInfo, OutputScriptInfo, TransactionInfo};

pub fn subcommand<'a>() -> clap::App<'a, 'a> {
	cmd::subcommand("create", "create a raw transaction from JSON").args(&[
		cmd::arg("tx-info", "the transaction info in JSON").required(true),
		cmd::opt("raw-stdout", "output the raw bytes of the result to stdout")
			.short("r")
			.required(false),
	])
	.long_about(r#"
Create a transaction from JSON. Use the same format as the `hal tx decode` output.

Example format:
{
  "version": 1,
  "locktime": 0,
  "inputs": [
    {
      "prevout": "78a0f5b35b73f1f6e054274aa3904867774600f09bd194e97e7a0fd953b27c54:6",
      "script_sig": {
        "hex":
"483045022100fad8d9b44d1d3a86bd9719ef642b32ed0a1c8f4e3de4e2009936988f73f12ad702207a2204cbdfd166d099cbb08e6c7886db5b986ef4fdfee383c1b8fc4df82ecea80121030a696d89d161c086586cf0de7d98fb97181a1ee0265130f7ddbecd17d616c780"
      },
      "sequence": 4294967295
    },
    {
      "txid": "c182fa9182957c5b906fd2b339d7a01dd110340bced99e049e2bd2c135f4513a",
      "vout": 1,
      "script_sig": {
        "hex": "220020fa28dc1e5eb222055e90f8cade9bcd13ca9ddab7a5ed029e27d41a736f7455ce"
      },
      "sequence": 4294967294,
      "witness": [
        "",
        "30440220725e1c098d85013166fae52794811f6531ff3962ea6bc3228ecfdd4699ae669b022064d5c88f2b838968a345681bbfeb2c09f0433ece511bc4d139c4805adf59d74601",
        "3044022055aa0f675bf0c21e113527f838b93d5922143ae6e52b094416d44551ff6d236202205ef3773cc9a7fe2076310c92adc73670747309265ecedb0cffe194885a89863601",
        "5221027111c0d6cbc3a40c6e6197ed234bd6e59f277c88094fd33297b1e0a3787a5b7d2102e71711c9840d68e6401d4bd5df78f1850e25ae41f082f4b38ceec37d60cab5442103eeae18900c0d12046f644b960a1ef84589f7f4f71d07914006d550bf85c576e153ae"
      ]
    }
  ],
  "outputs": [
    {
      "value": 500000,
      "script_pub_key": {
        "hex": "a91405394a3a5dedce4f945ed9f650fa9ff23f011d4687"
      }
    },
    {
      "value": 2590000,
      "script_pub_key": {
        "address": "34nFYcfPNTuWCV76YrwdVc4MyXmeVMMpsZ"
      }
    }
  ]
}"#
	)
}

/// Check both ways to specify the outpoint and panic if conflicting.
fn outpoint_from_input_info(input: &InputInfo) -> OutPoint {
	let op1 = input.prevout.as_ref().map(|ref op| op.parse().expect("invalid prevout format"));
	let op2 = match input.txid {
		Some(txid) => match input.vout {
			Some(vout) => Some(OutPoint {
				txid: txid,
				vout: vout,
			}),
			None => panic!("\"txid\" field given in input without \"vout\" field"),
		},
		None => None,
	};

	match (op1, op2) {
		(Some(op1), Some(op2)) => {
			if op1 != op2 {
				panic!("Conflicting prevout information in input.");
			}
			op1
		}
		(Some(op), None) => op,
		(None, Some(op)) => op,
		(None, None) => panic!("No previous output provided in input."),
	}
}

fn create_script_sig(ss: InputScriptInfo) -> Script {
	if let Some(hex) = ss.hex {
		if ss.asm.is_some() {
			warn!("Field \"asm\" of input is ignored.");
		}

		hex.0.into()
	} else if let Some(_) = ss.asm {
		panic!("Decoding script assembly is not yet supported.");
	} else {
		panic!("No scriptSig info provided.");
	}
}

fn create_input(input: InputInfo) -> TxIn {
	TxIn {
		previous_output: outpoint_from_input_info(&input),
		script_sig: input.script_sig.map(create_script_sig).unwrap_or_default(),
		sequence: input.sequence.unwrap_or_default(),
		witness: match input.witness {
			Some(ref w) => w.iter().map(|h| h.clone().0).collect(),
			None => Vec::new(),
		},
	}
}

fn create_script_pubkey(spk: OutputScriptInfo, used_network: &mut Option<Network>) -> Script {
	if spk.type_.is_some() {
		warn!("Field \"type\" of output is ignored.");
	}

	if let Some(hex) = spk.hex {
		if spk.asm.is_some() {
			warn!("Field \"asm\" of output is ignored.");
		}
		if spk.address.is_some() {
			warn!("Field \"address\" of output is ignored.");
		}

		//TODO(stevenroose) do script sanity check to avoid blackhole?
		hex.0.into()
	} else if let Some(_) = spk.asm {
		if spk.address.is_some() {
			warn!("Field \"address\" of output is ignored.");
		}

		panic!("Decoding script assembly is not yet supported.");
	} else if let Some(address) = spk.address {
		// Error if another network had already been used.
		if used_network.replace(address.network).unwrap_or(address.network) != address.network {
			panic!("Addresses for different networks are used in the output scripts.");
		}

		address.script_pubkey()
	} else {
		panic!("No scriptPubKey info provided.");
	}
}

fn create_output(output: OutputInfo) -> TxOut {
	// Keep track of which network has been used in addresses and error if two different networks
	// are used.
	let mut used_network = None;

	TxOut {
		value: output.value.expect("Field \"value\" is required for outputs."),
		script_pubkey: output
			.script_pub_key
			.map(|s| create_script_pubkey(s, &mut used_network))
			.unwrap_or_default(),
	}
}

pub fn create_transaction(info: TransactionInfo) -> Transaction {
	// Fields that are ignored.
	if info.txid.is_some() {
		warn!("Field \"txid\" is ignored.");
	}
	if info.hash.is_some() {
		warn!("Field \"hash\" is ignored.");
	}
	if info.size.is_some() {
		warn!("Field \"size\" is ignored.");
	}
	if info.weight.is_some() {
		warn!("Field \"weight\" is ignored.");
	}
	if info.vsize.is_some() {
		warn!("Field \"vsize\" is ignored.");
	}

	Transaction {
		version: info.version.expect("Field \"version\" is required."),
		lock_time: info.locktime.expect("Field \"locktime\" is required."),
		input: info
			.inputs
			.expect("Field \"inputs\" is required.")
			.into_iter()
			.map(create_input)
			.collect(),
		output: info
			.outputs
			.expect("Field \"outputs\" is required.")
			.into_iter()
			.map(create_output)
			.collect(),
	}
}

pub fn execute<'a>(matches: &clap::ArgMatches<'a>) {
	let json_tx = matches.value_of("tx-info").expect("no JSON tx info provided");
	let info: TransactionInfo = serde_json::from_str(json_tx).expect("invalid JSON");

	let tx = create_transaction(info);
	let tx_bytes = serialize(&tx);
	if matches.is_present("raw-stdout") {
		::std::io::stdout().write_all(&tx_bytes).unwrap();
	} else {
		print!("{}", hex::encode(&tx_bytes));
	}
}
