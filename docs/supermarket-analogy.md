# Supermarket analogy

Imagine a busy supermarket. Most of the time the rate of people arriving at the store is sustainable. People go in and come out again fairly quickly. Internally, queues are usually short.

At peak times, the number of people arriving every minute is too high. The cashiers can't keep up, queues start to form.

The supermarket has fancy _autoscaling_: as the queue sizes increase, more cashiers work the checkouts. There is, of course, a fixed number of cashiers and checkouts. They _could_ keep all checkouts open at all times to keep queues down and reduce people's shopping time, but this costs a lot of money. It's not worth it most of the time.

For a while, this is OK. But if the queues get too long, people get frustrated. Sometimes, after waiting too long they simply give up and leave without any shopping.

When it gets _really_ bad, the queues fill up the whole shop. People can't get in or out, everything gets stuck. People who do get in find a completely clogged shop and leave immediately.

Sometimes, capacity is reduced unexpectedly. Cashiers are sick, checkout registers aren't working.

To avoid this situation, a bouncer is hired to stand outside. The bouncer doesn't know what's going on inside the shop, because communication is bad. But they do know when people enter and exit.

After running a few tests, they observed that if more than ten people per minute enter the shop, things start to slow down, so they tell the bouncer to keep to this limit.

This kind of works, mostly. Except in some situations. Around Christmas it seems to be worse, and also when capacity is reduced.

After some investigation, they learn that around Christmas time, people take longer to shop. Instead of customers doing their normal shop where they get the same things every week, they do a bigger shop which takes longer. They do some measurements and find that on average customers take 10 minutes to do a normal shop, but at Christmas it takes 15 minutes.

They do some little calculations: 10 people per minute \* 10 minutes each = 100 people in the shop at once. 10 people per minute \* 15 minutes each = 150 people in the shop at once. This is why everything is getting clogged up.

So they change their approach. Instead of letting in 10 people per minute, they let a maximum of 100 people in the shop at a time.

This makes things better. Even when people take longer in the shop, they don't get long queues like before. It also helps with the capacity problem, for the most part. On days with fewer cashiers, checking out takes a bit longer. When they reach the 100 person limit, it's a one-in-one-out situation. If people spend longer in the shop, fewer people per minute are let in.

One day though, there's a problem with the milk. Normally there's two shelves, two meters wide, full of milk. On busy days, several people can pick up milk at the same time and move on. But not today: some of the fridges aren't working. Everything gets squeezed into the available fridge space, and there's only room for one person to pick up milk at once. A queue starts to form.

The layout of the shop is designed to handle queues near the checkouts, but not the milk. As the queue gets bigger, it blocks the whole aisle. Even people who don't need milk are getting blocked. Everything slows down. Those who manage to finish their shopping spent much longer in the shop. Many people leave without finishing. Those who leave are quickly replaced. The queue doesn't go down.

Soon, the milk on the shelf runs out. Someone needs to restock it, but they can't get access because they're blocked by the queue. It's a deadlock.

The shop managers have another think. Even by limiting the number of people in the shop, they couldn't avoid it getting clogged up. But, they think, maybe the bouncer could have been smarter. The bouncer could have observed that shoppers were taking longer, and that many were leaving without shopping. This might be a sign that things aren't going well inside the shop, and fewer people should be let inside.

They start writing some algorithms. In general: when people take longer in the shop, or people start leaving without any shopping, the limit is reduced. When the time spent in the shop goes down again, or people stop leaving empty-handed, the limit gets increased again.

This works well, and is so successful they want to launch it in their flagship shop. This shop is so large, it has two entrances: one for pedestrians and one for cars.

They wonder if this might pose a problem. Their bouncers, already known for not communicating with shop staff, also won't communicate with each other. Will this system still work?

They try it. They funnel as many people as they can through entrance one, until the shop is at capacity. This is working as before. Then, they open the second entrance. Both bouncers start seeing increased wait times, and people leaving without any shopping. They reduce their limits. Eventually, they both settle to around the same number. The system works.
