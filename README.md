# Grader

This is a application meant to quickly grade programming assignments submitted to Canvas.

As a note the application will load a .env file in the current working directory.

It is very fragile and expects a few things:

1. env var CANVAS_BASE_URL set to the base url of canvas with a trailing slash.
1. env var CANVAS_ACCESS_TOKEN set to an api access token
1. only one zip attachment to a submission
1. .c and .h extensions for the source code

The currently implemented features:

- select course
- select assignment
- divide into arbitrary portions (last portion gets remainder)
- sub selection in the portion (for graceful resume)
- fetch the zip submissions
- extract the zips
- open editor in each .c, .h, makefile, and readme
- spawn a shell in the downloaded dir (for building and running)

There is no smart analysis of the code. This is quite possible given I found a c parser for rust.
