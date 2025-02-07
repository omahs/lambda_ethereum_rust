// Exchange Operations (16)
// Opcodes: SWAP1 ... SWAP16
use super::*;

impl VM {
    // SWAP operation
    pub fn op_swap(
        &mut self,
        current_call_frame: &mut CallFrame,
        op: Opcode,
    ) -> Result<OpcodeSuccess, VMError> {
        if self.env.consumed_gas + gas_cost::SWAPN > self.env.tx_env.gas_limit {
            return Err(VMError::OutOfGas);
        }
        let depth = (op as u8) - (Opcode::SWAP1 as u8) + 1;

        if current_call_frame.stack.len() < depth as usize {
            return Err(VMError::StackUnderflow);
        }
        let stack_top_index = current_call_frame.stack.len();
        let to_swap_index = stack_top_index
            .checked_sub(depth as usize)
            .ok_or(VMError::StackUnderflow)?;
        current_call_frame
            .stack
            .swap(stack_top_index - 1, to_swap_index - 1);
        self.env.consumed_gas += gas_cost::SWAPN;

        Ok(OpcodeSuccess::Continue)
    }
}
