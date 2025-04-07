# order match contract

limit order match contract

```
pub enum ExchangeFunction {
    OrderMatch = 0x00,
    Refund = 0x01
    Withdraw = 0x02,
}

```

## OrderMatch

OrderMatch transfer funds to the exchange wallet, mint an Order. an order book can match two orders, make swap, send shares back to LPs.

```
circuit "Order" {
    # Poseidon hash of the order
    O = poseidon_hash(
        withdraw_public_x,
        withdraw_public_y,
        base_value,
        quote_value,
        base_token_id,
        quote_token_id,
        spend_hook,
        user_data,
        order_blind,
        timeout_duration_blind,
    );
    constrain_instance(O);

    # Pedersen commitment for order's base order_value
    base_vcv = ec_mul_short(base_value, VALUE_COMMIT_VALUE);
    base_vcr = ec_mul(base_value_blind, VALUE_COMMIT_RANDOM);
    base_order_value_commit = ec_add(base_vcv, base_vcr);
    # Since the base_value commit is a curve point, we fetch its coordinates
    # and constrain them:
    constrain_instance(ec_get_x(base_order_value_commit));
    constrain_instance(ec_get_y(base_order_value_commit));

    # Pedersen commitment for order's quote_value
    quote_vcv = ec_mul_short(quote_value, VALUE_COMMIT_VALUE);
    quote_vcr = ec_mul(quote_value_blind, VALUE_COMMIT_RANDOM);
    quote_order_value_commit = ec_add(quote_vcv, quote_vcr);
    # Since the quote_value commit is a curve point, we fetch its coordinates
    # and constrain them:
    constrain_instance(ec_get_x(quote_order_value_commit));
    constrain_instance(ec_get_y(quote_order_value_commit));

    # Commitment for order's base_token_id ID. We do a poseidon hash since it's
    # cheaper than EC operations and doesn't need the homomorphic prop.
    base_order_token_id_commit = poseidon_hash(base_token_id, base_token_id_blind);
    constrain_instance(base_order_token_id_commit);

    # Commitment for order's quote_token_id ID. We do a poseidon hash since it's
    # cheaper than EC operations and doesn't need the homomorphic prop.
    quote_order_token_id_commit = poseidon_hash(quote_token_id, quote_token_id_blind);
    constrain_instance(quote_order_token_id_commit);

    # Commitment for order's timeout_duration.  We do a poseidon hash since it's
    # cheaper than EC operations and doesn't need the homomorphic prop.
    timeout_duration_commit = poseidon_hash(timeout_duration, timeout_duration_blind);
    constrain_instance(timeout_duration_commit):

    # At this point we've enforced all of our public inputs.
}

```

## swap

once a match is found in the order book, the exchange perform the swap, having a valid swap is a proof that there are a matching orders.


## overview on liquidity ownership

the following is a trace of the liquidity ownership of the current implementation see `./tests/exchange_swap.rs`:
- LP mint and order, and transfer liquidity to exchange.
- find a matching order
- perform full swap
- transfer the liquidity back to the initial LP

## TODO Refund call (prevent the exchange from running away with the funds)

create transfer call similar to `Money::Transfer` with minor difference that allow burning coin in the exchange possession with spend_hook set to exchange_conract_id, and mint a LP's coin with None/Money contract id set to it's spend_hook.

## TODO withdraw call

withdraw call transfer funds in exchange possession with limited use inside the Exchange contract back to the liquidity provider in case of:
- order time out
- cancel a transaction


## TODO spread difference

the spread should be added to the book, commit to the spread difference value, it will be spent along with the fee transaction upon swap success.
