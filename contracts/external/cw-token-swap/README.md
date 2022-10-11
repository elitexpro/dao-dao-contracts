# cw-token-swap

This is an escrow token swap contract for swapping between native and
cw20 tokens. The contract is instantiated with two counterparties and
their promised funds. Promised funds may either be native tokens or
cw20 tokens. Upon both counterparties providing the promised funds the
transaction is completed and both sides receive their tokens.

At any time before the other counterparty has provided funds a
counterparty may withdraw their funds.

