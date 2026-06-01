# Brain Dump

File holding random notes and thoughts from humans while AI is implementing.

## When To Add Outbox?

We need outbox/pubsub basically when we're going to do virus scanning so add it after the quarantine
uploads are added. Put 017 on hold, do the outbox/pubsub stuff, then come back.

## Fleshing Out Authorization

I wonder how Authzed can be used to create a system where different actors actually can get different
permissions that are set in the platform as opposed to schematically. For example, a human user should
be able to give an AI agent actor the ability to read and even write evidence requests but keep them
from submitting the evidence, instead keeping that permission for their corresponding human actor.
