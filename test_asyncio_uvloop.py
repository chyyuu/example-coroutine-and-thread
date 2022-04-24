import asyncio
import uvloop
import aiohttp

from timer import timer

URL = 'https://httpbin.org/uuid'


async def fetch(session, url):
    async with session.get(url) as response:
        json_response = await response.json()
        print(json_response['uuid'])


async def main():
    async with aiohttp.ClientSession() as session:
        tasks = [fetch(session, URL) for _ in range(100)]
        await asyncio.gather(*tasks)


@timer(1, 1)
def func():
    uvloop.install()
    asyncio.run(main())
