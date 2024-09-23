deps:
	git clone https://github.com/starkware-libs/cairo.git \
	&& cd cairo \
	&& git checkout 93221753088d58f54f9a7f35a0bb338cf0bfb952 \
	&& cd .. \
	&& mv cairo/corelib/ . \
	&& rm -rf cairo/
