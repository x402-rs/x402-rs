use alloy_sol_types::sol;

sol!(
    #[allow(missing_docs)]
    #[allow(clippy::too_many_arguments)]
    #[derive(Debug)]
    #[sol(rpc)]
    IERC20,
    "abi/IERC20.json"
);
