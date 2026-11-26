#!/usr/bin/env python3
"""Train a LoRA adapter for layer 3 on the dataset of train_data.py.

designs/TRAINING.md §6. QLoRA: the base model is loaded in 4-bit, only
the adapter is trained, and the loss is on the answer only, so the model
learns what to answer to bodnuti's own request. Requests longer than
--max-tokens are left out. The newest examples (from --val-since) are
held back for validation.

On an AMD Radeon RX 7600 XT (gfx1102), bitsandbytes needs
HSA_OVERRIDE_GFX_VERSION=11.0.0.
"""

from __future__ import annotations

import argparse
import json
import random
from pathlib import Path

import torch
from datasets import Dataset
from peft import LoraConfig
from transformers import AutoTokenizer, BitsAndBytesConfig
from trl import SFTConfig, SFTTrainer


def load(paths: list[Path], tok, max_tokens: int, val_since: str):
    train, val, skipped = [], [], 0
    for p in paths:
        for line in p.open():
            ex = json.loads(line)
            msgs = ex["messages"]
            n = len(tok.apply_chat_template(msgs, tokenize=True))
            if n > max_tokens:
                skipped += 1
                continue
            row = {"prompt": msgs[:-1], "completion": msgs[-1:]}
            (val if ex["meta"]["date"] >= val_since else train).append(row)
    random.Random(1).shuffle(train)
    print(f"train {len(train)}, validation {len(val)}, left out as too long {skipped}", flush=True)
    return Dataset.from_list(train), Dataset.from_list(val)


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--data", type=Path, nargs="+", required=True)
    ap.add_argument("--model", default="google/gemma-4-12B-it")
    ap.add_argument("--out", type=Path, required=True)
    ap.add_argument("--max-tokens", type=int, default=8192)
    ap.add_argument("--val-since", default="2025-07-01")
    ap.add_argument("--epochs", type=float, default=2)
    ap.add_argument("--max-steps", type=int, default=-1, help="for a trial run")
    ap.add_argument("--rank", type=int, default=16)
    ap.add_argument("--lr", type=float, default=1e-4)
    args = ap.parse_args()

    tok = AutoTokenizer.from_pretrained(args.model)
    train, val = load(args.data, tok, args.max_tokens, args.val_since)

    config = SFTConfig(
        output_dir=str(args.out),
        num_train_epochs=args.epochs,
        max_steps=args.max_steps,
        per_device_train_batch_size=1,
        gradient_accumulation_steps=8,
        learning_rate=args.lr,
        lr_scheduler_type="cosine",
        warmup_ratio=0.05,
        bf16=True,
        gradient_checkpointing=True,
        max_length=args.max_tokens,
        logging_steps=5,
        eval_strategy="steps" if len(val) else "no",
        eval_steps=50,
        per_device_eval_batch_size=1,
        save_steps=50,
        save_total_limit=3,
        report_to="none",
        model_init_kwargs={
            "quantization_config": BitsAndBytesConfig(
                load_in_4bit=True,
                bnb_4bit_quant_type="nf4",
                bnb_4bit_compute_dtype=torch.bfloat16,
                bnb_4bit_use_double_quant=True,
            ),
            "dtype": torch.bfloat16,
            "device_map": {"": 0},
        },
    )
    lora = LoraConfig(
        r=args.rank,
        lora_alpha=2 * args.rank,
        lora_dropout=0.05,
        target_modules=["q_proj", "k_proj", "v_proj", "o_proj", "gate_proj", "up_proj", "down_proj"],
        task_type="CAUSAL_LM",
    )
    trainer = SFTTrainer(
        model=args.model,
        args=config,
        train_dataset=train,
        eval_dataset=val if len(val) else None,
        processing_class=tok,
        peft_config=lora,
    )
    trainer.train()
    trainer.save_model(str(args.out / "adapter"))
    print("peak GPU memory GB", round(torch.cuda.max_memory_allocated() / 1e9, 1))


if __name__ == "__main__":
    main()
